use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    config::{AppConfig, ProviderManager},
    domain::{
        db,
        model::{Memory, Perception, Topic},
    },
    error::{AppResultExt, ErrorKind, Result},
    util::pgvector,
};

const MAX_CONCURRENT: usize = 10;

struct Job {
    table: &'static str,
    id: Uuid,
    content: String,
}

pub async fn rebuild_embeddings(config: &AppConfig) -> Result<()> {
    let (mut db, _pool) = db::init_db(&config.database).await?;

    let providers = ProviderManager::new(config)?;
    let provider_name = config
        .multimodal
        .embedding
        .provider
        .as_deref()
        .unwrap_or(&config.agent.provider);
    let model = config
        .multimodal
        .embedding
        .model
        .as_deref()
        .unwrap_or("bge-m3");
    let dimension = config.multimodal.embedding.dimension.unwrap_or(1024);
    let agent = Arc::new(providers.build_agent(provider_name, model));

    pgvector::ensure_embedding_schema(&mut db, dimension).await?;

    for table in &["memory", "topic", "perception"] {
        toasty::sql::statement(format!("DROP INDEX IF EXISTS idx_{table}_embedding"))
            .exec(&mut db)
            .await?;
        toasty::sql::statement(format!("UPDATE {table} SET embedding = NULL"))
            .exec(&mut db)
            .await?;
    }

    let memories: Vec<Memory> = Memory::all()
        .exec(&mut db)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query memory")?;
    let topics: Vec<Topic> = Topic::filter(Topic::fields().summary().is_some())
        .exec(&mut db)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query topic")?;
    let perceptions: Vec<Perception> = Perception::all()
        .exec(&mut db)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query perception")?;

    let mut jobs = Vec::new();
    for m in &memories {
        jobs.push(Job {
            table: "memory",
            id: m.id,
            content: m.content.clone(),
        });
    }
    for t in &topics {
        if let Some(ref s) = t.summary
            && !s.is_empty()
        {
            jobs.push(Job {
                table: "topic",
                id: t.id,
                content: s.clone(),
            });
        }
    }
    for p in &perceptions {
        jobs.push(Job {
            table: "perception",
            id: p.id,
            content: p.content.clone(),
        });
    }

    let total = jobs.len();
    let pb = progress_bar("embeddings", total);
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let mut failed = 0usize;

    let mut stream: FuturesUnordered<_> = jobs
        .into_iter()
        .map(|job| {
            let agent = Arc::clone(&agent);
            let sem = Arc::clone(&sem);
            let mut db = db.clone();
            async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| ErrorKind::Internal.msg("semaphore closed"))?;
                let emb = agent.embedding(&job.content).await.map_err(|e| {
                    ErrorKind::Internal.msg(format!("{}/{}: {e}", job.table, job.id))
                })?;
                let sql = format!(
                    "UPDATE {t} SET embedding = '{v}'::vector WHERE id = '{id}'",
                    t = job.table,
                    v = pgvector::vec_to_pgstring(&emb),
                    id = job.id,
                );
                toasty::sql::statement(sql).exec(&mut db).await?;
                Ok::<_, crate::error::AppError>(())
            }
        })
        .collect();

    while let Some(result) = stream.next().await {
        match result {
            Ok(()) => {}
            Err(e) => {
                tracing::warn!("{e}");
                failed += 1;
            }
        }
        pb.inc(1);
    }

    pb.finish_and_clear();
    let ok = total - failed;
    println!("Written {ok}/{total} embeddings.");
    if failed > 0 {
        return Err(ErrorKind::Internal.msg(format!("{failed}/{total} embeddings failed")));
    }

    for table in &["memory", "topic", "perception"] {
        let lists = (10usize).max((total as f64).sqrt() as usize);
        toasty::sql::statement(format!(
            "CREATE INDEX IF NOT EXISTS idx_{table}_embedding ON {table} \
             USING ivfflat (embedding vector_cosine_ops) WITH (lists = {lists})"
        ))
        .exec(&mut db)
        .await?;
    }

    println!("Done. Rebuilt embeddings ({dimension}d) using {provider_name}/{model}.");
    Ok(())
}

fn progress_bar(name: &str, total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("{prefix:>12} {bar:30.green} {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("━ "),
    );
    pb.set_prefix(name.to_owned());
    pb
}
