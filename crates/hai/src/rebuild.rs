use std::sync::Arc;

use futures::stream::{FuturesUnordered, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    agentcore::{ApiClient, Endpoint},
    config::{AppConfig, ProviderRegistry},
    domain::{
        db,
        model::{KnowledgeChunk, Memory, Perception, Topic},
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
    let pool = db::init_db(&config.database).await?;
    let registry = ProviderRegistry::new(config)?;

    let dimension = config
        .auxiliary
        .embedding
        .as_ref()
        .map(|b| b.dimension())
        .unwrap_or(1024);
    let role = config.auxiliary.embedding.as_ref();
    let provider: &str = role
        .map(|b| {
            b.provider
                .as_deref()
                .unwrap_or(config.agent.provider.as_str())
        })
        .unwrap_or(config.agent.provider.as_str());
    let model: &str = role.map(|b| b.model.as_deref().unwrap_or("")).unwrap_or("");

    let client = Arc::new(ApiClient::new());
    let ep = Arc::new(registry.get_endpoint(provider, model)?);

    pgvector::ensure_embedding_schema(&pool, dimension).await?;
    reset_embeddings(&pool).await?;

    let jobs = collect_jobs(&pool).await?;
    let total = jobs.len();
    let pb = progress_bar("embeddings", total);
    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT));

    let failed = run_batch(client, ep, jobs, pool.clone(), sem, &pb).await;

    pb.finish_and_clear();
    let ok = total.saturating_sub(failed);
    println!("Written {ok}/{total} embeddings.");

    rebuild_indexes(&pool, total).await?;
    println!("Done. Rebuilt embeddings ({dimension}d) using {provider}/{model}.");

    if failed > 0 {
        Err(ErrorKind::Internal.msg(format!("{failed}/{total} embeddings failed")))
    } else {
        Ok(())
    }
}

async fn reset_embeddings(pool: &PgPool) -> Result<()> {
    for table in &["memory", "topic", "perception", "knowledge_chunk"] {
        sqlx::query(sqlx::AssertSqlSafe(
            format!("DROP INDEX IF EXISTS idx_{table}_embedding").as_str(),
        ))
        .execute(pool)
        .await?;
        sqlx::query(sqlx::AssertSqlSafe(
            format!("UPDATE {table} SET embedding = NULL").as_str(),
        ))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn collect_jobs(pool: &PgPool) -> Result<Vec<Job>> {
    let memories: Vec<Memory> = sqlx::query_as::<_, Memory>("SELECT * FROM memory")
        .fetch_all(pool)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query memory")?;
    let topics: Vec<Topic> =
        sqlx::query_as::<_, Topic>("SELECT * FROM topic WHERE summary IS NOT NULL")
            .fetch_all(pool)
            .await
            .err_kind_msg(ErrorKind::Internal, "Failed to query topic")?;
    let perceptions: Vec<Perception> = sqlx::query_as::<_, Perception>("SELECT * FROM perception")
        .fetch_all(pool)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query perception")?;
    let chunks: Vec<KnowledgeChunk> =
        sqlx::query_as::<_, KnowledgeChunk>("SELECT * FROM knowledge_chunk")
            .fetch_all(pool)
            .await
            .err_kind_msg(ErrorKind::Internal, "Failed to query knowledge_chunk")?;

    let mut jobs =
        Vec::with_capacity(memories.len() + topics.len() + perceptions.len() + chunks.len());
    for m in &memories {
        jobs.push(Job {
            table: "memory",
            id: m.id,
            content: m.content.clone(),
        });
    }
    for t in &topics {
        if let Some(s) = &t.summary
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
    for c in &chunks {
        jobs.push(Job {
            table: "knowledge_chunk",
            id: c.id,
            content: c.content.clone(),
        });
    }
    Ok(jobs)
}

async fn run_batch(
    client: Arc<ApiClient>,
    ep: Arc<Endpoint>,
    jobs: Vec<Job>,
    pool: PgPool,
    sem: Arc<Semaphore>,
    pb: &ProgressBar,
) -> usize {
    let mut stream: FuturesUnordered<_> = jobs
        .into_iter()
        .map(move |job| {
            let client = Arc::clone(&client);
            let ep = Arc::clone(&ep);
            let sem = Arc::clone(&sem);
            let pool = pool.clone();
            async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|_| ErrorKind::Internal.msg("semaphore closed"))?;
                let emb = client.embed(&ep, &job.content).await.map_err(|e| {
                    ErrorKind::Internal.msg(format!("{}/{}: {e}", job.table, job.id))
                })?;
                sqlx::query(sqlx::AssertSqlSafe(
                    format!(
                        "UPDATE {t} SET embedding = $1::vector WHERE id = $2",
                        t = job.table,
                    )
                    .as_str(),
                ))
                .bind(pgvector::vec_to_pgstring(&emb))
                .bind(job.id)
                .execute(&pool)
                .await?;
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

async fn rebuild_indexes(pool: &PgPool, total: usize) -> Result<()> {
    let lists = (10usize).max((total as f64).sqrt() as usize);
    for table in &["memory", "topic", "perception", "knowledge_chunk"] {
        // 先 DROP 再建：旧索引若为 cosine_ops（与 `<->` L2 查询不匹配）需重建；
        // ivfflat 要求表有数据——调用方保证向量已写入（run_batch 之后）。
        let idx_sql = format!(
            "DROP INDEX IF EXISTS idx_{table}_embedding; \
             CREATE INDEX idx_{table}_embedding ON {table} \
             USING ivfflat (embedding vector_l2_ops) WITH (lists = {lists})"
        );
        sqlx::query(sqlx::AssertSqlSafe(idx_sql.as_str()))
            .execute(pool)
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
