use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    agentcore::{ApiClient, Endpoint},
    config::{AppConfig, ProviderRegistry},
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
    let registry = ProviderRegistry::new(config)?;

    let ec = &config.multimodal.embedding;
    let provider = ec.provider(&config.agent.provider);
    let model = ec.model();
    let dimension = ec.dimension();

    let client = Arc::new(ApiClient::new());
    let ep = Arc::new(registry.resolve(&provider, &model)?);

    pgvector::ensure_embedding_schema(&mut db, dimension).await?;
    reset_embeddings(&mut db).await?;

    let jobs = collect_jobs(&db).await?;
    let total = jobs.len();
    let pb = progress_bar("embeddings", total);
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT));

    let failed = run_batch(client, ep, jobs, db.clone(), sem, &pb).await;

    pb.finish_and_clear();
    let ok = total.saturating_sub(failed);
    println!("Written {ok}/{total} embeddings.");

    rebuild_indexes(&mut db, total).await?;
    println!("Done. Rebuilt embeddings ({dimension}d) using {provider}/{model}.");

    if failed > 0 {
        Err(ErrorKind::Internal.msg(format!("{failed}/{total} embeddings failed")))
    } else {
        Ok(())
    }
}

async fn reset_embeddings(db: &mut toasty::Db) -> Result<()> {
    for table in &["memory", "topic", "perception"] {
        toasty::sql::statement(format!("DROP INDEX IF EXISTS idx_{table}_embedding"))
            .exec(db)
            .await?;
        toasty::sql::statement(format!("UPDATE {table} SET embedding = NULL"))
            .exec(db)
            .await?;
    }
    Ok(())
}

async fn collect_jobs(db: &toasty::Db) -> Result<Vec<Job>> {
    let memories: Vec<Memory> = Memory::all()
        .exec(&mut db.clone())
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query memory")?;
    let topics: Vec<Topic> = Topic::filter(Topic::fields().summary().is_some())
        .exec(&mut db.clone())
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query topic")?;
    let perceptions: Vec<Perception> = Perception::all()
        .exec(&mut db.clone())
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query perception")?;

    let mut jobs = Vec::with_capacity(memories.len() + topics.len() + perceptions.len());
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
    Ok(jobs)
}

async fn run_batch(
    client: Arc<ApiClient>,
    ep: Arc<Endpoint>,
    jobs: Vec<Job>,
    db: toasty::Db,
    sem: Arc<Semaphore>,
    pb: &ProgressBar,
) -> usize {
    let mut stream: FuturesUnordered<_> = jobs
        .into_iter()
        .map(move |job| {
            let client = Arc::clone(&client);
            let ep = Arc::clone(&ep);
            let sem = Arc::clone(&sem);
            let mut db = db.clone();
            async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| ErrorKind::Internal.msg("semaphore closed"))?;
                let emb = client.embed(&ep, &job.content).await.map_err(|e| {
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

    let mut failed = 0usize;
    while let Some(result) = stream.next().await {
        if let Err(e) = result {
            tracing::warn!("{e}");
            failed += 1;
        }
        pb.inc(1);
    }
    failed
}

async fn rebuild_indexes(db: &mut toasty::Db, total: usize) -> Result<()> {
    let lists = (10usize).max((total as f64).sqrt() as usize);
    for table in &["memory", "topic", "perception"] {
        toasty::sql::statement(format!(
            "CREATE INDEX IF NOT EXISTS idx_{table}_embedding ON {table} \
             USING ivfflat (embedding vector_cosine_ops) WITH (lists = {lists})"
        ))
        .exec(db)
        .await?;
    }
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
