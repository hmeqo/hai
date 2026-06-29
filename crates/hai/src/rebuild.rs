use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    agentcore::RawAgent,
    config::{AppConfig, ProviderManager},
    domain::{
        db,
        model::{Memory, Perception, Topic},
    },
    error::{AppResultExt, ErrorKind, Result},
};

pub async fn rebuild_embeddings(config: &AppConfig) -> Result<()> {
    let mut pool = db::init_db(&config.database).await?;
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
    let agent = providers.build_agent(provider_name, model);

    let mut total = 0usize;

    let memories: Vec<Memory> = Memory::all()
        .exec(&mut pool)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query memory")?;
    total += rebuild_memories(&agent, &pool, &memories).await?;

    let topics: Vec<Topic> = Topic::filter(Topic::fields().summary().is_some())
        .exec(&mut pool)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query topic")?;
    total += rebuild_topics(&agent, &pool, &topics).await?;

    let perceptions: Vec<Perception> = Perception::all()
        .exec(&mut pool)
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to query perception")?;
    total += rebuild_perceptions(&agent, &pool, &perceptions).await?;

    println!("Done. Rebuilt {total} embeddings using {provider_name}/{model}.");
    Ok(())
}

async fn rebuild_memories(agent: &RawAgent, pool: &toasty::Db, rows: &[Memory]) -> Result<usize> {
    let pb = progress_bar("memory", rows.len());
    let mut ok = 0usize;
    for row in rows {
        match agent.embedding(&row.content).await {
            Ok(emb) => {
                Memory::filter_by_id(row.id)
                    .update()
                    .embedding(toasty::Json(emb))
                    .exec(&mut pool.clone())
                    .await?;
                ok += 1;
            }
            Err(e) => tracing::warn!("memory {}: embedding failed, skipping: {e}", row.id),
        }
        pb.inc(1);
    }
    pb.finish_and_clear();
    Ok(ok)
}

async fn rebuild_topics(agent: &RawAgent, pool: &toasty::Db, rows: &[Topic]) -> Result<usize> {
    let pb = progress_bar("topic", rows.len());
    let mut ok = 0usize;
    for row in rows {
        let content = row.summary.as_deref().unwrap_or("");
        match agent.embedding(content).await {
            Ok(emb) => {
                Topic::filter_by_id(row.id)
                    .update()
                    .embedding(Some(toasty::Json(emb)))
                    .exec(&mut pool.clone())
                    .await?;
                ok += 1;
            }
            Err(e) => tracing::warn!("topic {}: embedding failed, skipping: {e}", row.id),
        }
        pb.inc(1);
    }
    pb.finish_and_clear();
    Ok(ok)
}

async fn rebuild_perceptions(
    agent: &RawAgent,
    pool: &toasty::Db,
    rows: &[Perception],
) -> Result<usize> {
    let pb = progress_bar("perception", rows.len());
    let mut ok = 0usize;
    for row in rows {
        match agent.embedding(&row.content).await {
            Ok(emb) => {
                Perception::filter_by_id(row.id)
                    .update()
                    .embedding(Some(toasty::Json(emb)))
                    .exec(&mut pool.clone())
                    .await?;
                ok += 1;
            }
            Err(e) => tracing::warn!("perception {}: embedding failed, skipping: {e}", row.id),
        }
        pb.inc(1);
    }
    pb.finish_and_clear();
    Ok(ok)
}

fn progress_bar(name: &str, total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("{prefix:12} [{bar:40.cyan/blue}] {pos:>5}/{len:<5} ({eta})")
            .unwrap()
            .progress_chars("█░"),
    );
    pb.set_prefix(name.to_owned());
    pb
}
