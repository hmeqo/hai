//! `hai kb` 子命令：知识库管理（CLI 入口，后台管理面）。

use std::path::{Path, PathBuf};

use crate::{
    agent::multimodal::MultimodalService,
    config::{AppConfig, ProviderRegistry},
    domain::{db, repo::Repos, service::DbServices, vo::KnowledgeDocumentId},
    error::{ErrorKind, Result},
    util::chunking::ChunkCfg,
};

/// 支持的导入扩展名（首版：文本/Markdown）。
const IMPORT_EXTENSIONS: [&str; 2] = ["md", "txt"];

/// 构建知识库操作所需的 service 装配（对齐 AppContext 装配顺序）。
pub async fn build_services(config: &AppConfig) -> Result<DbServices> {
    let registry = ProviderRegistry::new(config)?;
    let multimodal = MultimodalService::from_config(config, &registry)?;
    let pool = db::init_db(&config.database).await?;
    Ok(DbServices::new(Repos::new(pool), multimodal))
}

pub fn chunk_cfg(config: &AppConfig) -> ChunkCfg {
    ChunkCfg {
        size: config.knowledge.chunk_size,
        overlap: config.knowledge.chunk_overlap,
        max: config.knowledge.chunk_max,
    }
}

pub async fn execute(action: super::KbAction, config: &AppConfig) -> Result<()> {
    let services = build_services(config).await?;
    let cfg = chunk_cfg(config);
    match action {
        super::KbAction::Import {
            paths,
            collection,
            title,
            recursive,
        } => {
            import(
                &services,
                &paths,
                collection.as_deref(),
                title.as_deref(),
                recursive,
                &cfg,
            )
            .await
        }
        super::KbAction::List { collection } => list(&services, collection.as_deref()).await,
        super::KbAction::Search {
            query,
            collection,
            limit,
        } => search(&services, &query, collection.as_deref(), limit).await,
        super::KbAction::Delete { id } => {
            services.knowledge.delete(KnowledgeDocumentId(id)).await?;
            println!("Deleted document {id}");
            Ok(())
        }
        super::KbAction::Reindex { collection } => {
            let n = services
                .knowledge
                .reindex(collection.as_deref(), &cfg)
                .await?;
            println!("Reindexed {n} document(s).");
            Ok(())
        }
    }
}

// ── import ──────────────────────────────────────────────────────────────────

async fn import(
    services: &DbServices,
    paths: &[PathBuf],
    collection: Option<&str>,
    title_override: Option<&str>,
    recursive: bool,
    cfg: &ChunkCfg,
) -> Result<()> {
    let files = collect_files(paths, recursive)?;
    if files.is_empty() {
        return Err(ErrorKind::BadRequest.msg("no importable files (.md/.txt) found"));
    }

    let bar = if files.len() > 1 {
        Some(indicatif::ProgressBar::new(files.len() as u64))
    } else {
        None
    };

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, String)> = Vec::new();

    for path in &files {
        let result = import_one(services, path, collection, title_override, cfg).await;
        match result {
            Ok(true) => imported += 1,
            Ok(false) => skipped += 1,
            Err(e) => failures.push((path.display().to_string(), e.to_string())),
        }
        if let Some(b) = &bar {
            b.inc(1);
        }
    }
    if let Some(b) = bar {
        b.finish_and_clear();
    }

    println!(
        "Imported {imported}, skipped {skipped} (unchanged), failed {}.",
        failures.len()
    );
    for (path, err) in &failures {
        println!("  failed: {path}: {err}");
    }
    if !failures.is_empty() {
        // 单文件失败多为用户输入问题（不可读/非 UTF-8），聚合为 BadRequest
        // （Internal 会命中 is_internal_error → tracing::error!，把用户错误当内部故障刷日志）
        return Err(
            ErrorKind::BadRequest.msg(format!("{} file(s) failed to import", failures.len()))
        );
    }
    Ok(())
}

/// 导入单个文件。返回 true = 导入/更新，false = 内容未变跳过。
async fn import_one(
    services: &DbServices,
    path: &Path,
    collection: Option<&str>,
    title_override: Option<&str>,
    cfg: &ChunkCfg,
) -> Result<bool> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ErrorKind::DataParse.msg(format!("cannot read {}: {e}", path.display())))?;
    let (front_title, body) = extract_frontmatter(&content);
    let title = title_override
        .map(String::from)
        .or(front_title)
        .unwrap_or_else(|| stem_name(path));
    let source = path.display().to_string();
    let outcome = services
        .knowledge
        .upsert_document(&source, &title, collection.unwrap_or(""), &body, cfg, false)
        .await?;
    if outcome.imported {
        println!("  imported: {title} ({} chunks)", outcome.chunk_count);
    }
    Ok(outcome.imported)
}

/// 收集待导入文件：文件直接取；目录需 `--recursive`（显式防误导入），
/// 递归收集目录树下的 .md/.txt。
fn collect_files(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        let meta = std::fs::metadata(p).map_err(|e| {
            ErrorKind::BadRequest.msg(format!("cannot access {}: {e}", p.display()))
        })?;
        if meta.is_file() {
            if !is_importable(p) {
                return Err(ErrorKind::BadRequest.msg(format!(
                    "'{}' is not an importable file (.md/.txt)",
                    p.display()
                )));
            }
            files.push(p.clone());
        } else if meta.is_dir() {
            if !recursive {
                return Err(ErrorKind::BadRequest.msg(format!(
                    "'{}' is a directory; use --recursive to import recursively",
                    p.display()
                )));
            }
            walk_dir(p, &mut files)?;
        }
    }
    // 按路径排序，导入顺序稳定
    files.sort();
    Ok(files)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| ErrorKind::BadRequest.msg(format!("cannot read dir {}: {e}", dir.display())))?
    {
        let entry = entry.map_err(|e| ErrorKind::BadRequest.msg(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if is_importable(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_importable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMPORT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn stem_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string()
}

/// 提取 Markdown frontmatter 的 title（零依赖手写解析）；
/// 返回 (title, 剥离 frontmatter 后的正文)。无 frontmatter → (None, 原文)。
///
/// 要求 head 内存在 `key: value` 结构才视为 frontmatter（文件顶部的 `---`
/// 也可能是水平线分隔，避免误剥离正文）。
fn extract_frontmatter(content: &str) -> (Option<String>, String) {
    let Some(rest) = content.strip_prefix("---") else {
        return (None, content.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (None, content.to_string());
    };
    let head = &rest[..end];
    if !head.lines().any(|l| l.split_once(':').is_some()) {
        return (None, content.to_string());
    }
    let title = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim() == "title" {
            Some(v.trim().trim_matches('"').trim_matches('\'').to_string())
        } else {
            None
        }
    });
    // `\n---` 为 4 字节；end 指向 `\n` 的下标
    let body_start = end + 4;
    let body = rest.get(body_start..).unwrap_or("");
    (title, body.trim_start_matches('\n').to_string())
}

// ── list / search ───────────────────────────────────────────────────────────

async fn list(services: &DbServices, collection: Option<&str>) -> Result<()> {
    let docs = services.knowledge.list(collection).await?;
    if docs.is_empty() {
        println!("(empty)");
        return Ok(());
    }
    for doc in docs {
        let col = if doc.collection.is_empty() {
            "-"
        } else {
            &doc.collection
        };
        println!("{:<38}  {:<24}  {}", doc.id, col, doc.title);
    }
    Ok(())
}

async fn search(
    services: &DbServices,
    query: &str,
    collection: Option<&str>,
    limit: i64,
) -> Result<()> {
    let collections: Vec<String> = collection.map(|c| vec![c.to_string()]).unwrap_or_default();
    let hits = services
        .knowledge
        .search(query, limit, &collections)
        .await?;
    if hits.is_empty() {
        println!("(no matches)");
        return Ok(());
    }
    for hit in hits {
        let col = if hit.collection.is_empty() {
            "-"
        } else {
            &hit.collection
        };
        let snippet: String = hit.content.chars().take(120).collect();
        println!(
            "[{col}] {} (id={}, d={:.4})\n    {}",
            hit.document_title, hit.document_id.0, hit.distance, snippet
        );
    }
    Ok(())
}
