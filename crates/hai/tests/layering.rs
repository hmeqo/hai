//! The dependency edges `docs/architecture.md` declares, asserted rather than reviewed.
//!
//! A source file may name through `crate::` only the modules its layer allows, and the service layer
//! may name no SQL. The paths are read from the syntax tree, so a multi-line `use crate::{…}` cannot
//! slip past the way a text search would.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use syn::{
    ItemUse, Path as SynPath, UseTree,
    visit::{self, Visit},
};

/// What one top-level module of `src/` may name through `crate::`, beyond itself and the crate-root
/// `test_support`. This mirrors `docs/architecture.md`'s measured dependencies; a file outside its
/// layer's list fails.
const EDGES: &[(&str, &[&str])] = &[
    (
        "app",
        &[
            "agent",
            "agentcore",
            "config",
            "domain",
            "error",
            "platform",
            "util",
        ],
    ),
    (
        "platform",
        &["agent", "agentcore", "config", "domain", "error", "util"],
    ),
    ("agent", &["agentcore", "config", "domain", "error", "util"]),
    ("domain", &["agentcore", "config", "error"]),
    ("config", &["agentcore", "error"]),
    ("agentcore", &["error", "util"]),
    (
        "cli",
        &["agent", "agentcore", "app", "config", "domain", "error"],
    ),
    ("util", &["error"]),
];

/// One source file and the modules it names.
struct Source {
    path: PathBuf,
    /// The path below `src/`, which is where a layer is read from.
    relative: PathBuf,
    /// The first path component, when the file sits in a layer directory.
    layer: Option<String>,
    refs: Refs,
}

/// The modules one file names: the segment after `crate`, and the roots of every other path.
#[derive(Default)]
struct Refs {
    /// `crate::<segment>`.
    crate_segments: BTreeSet<String>,
    /// The first segment of every path that does not start at `crate`; this is where an extern
    /// crate such as `sqlx` shows up, including a macro's own path.
    extern_roots: BTreeSet<String>,
}

impl Refs {
    fn add_path(&mut self, path: &SynPath) {
        if path.leading_colon.is_none()
            && let Some(first) = path.segments.first()
        {
            match first.ident.to_string().as_str() {
                "crate" => {
                    if let Some(second) = path.segments.iter().nth(1) {
                        self.crate_segments.insert(second.ident.to_string());
                    }
                }
                "self" | "super" | "Self" => {}
                root => {
                    self.extern_roots.insert(root.to_owned());
                }
            }
        }
    }

    fn add_use(&mut self, tree: &UseTree, after_crate: bool) {
        match tree {
            UseTree::Path(path) => {
                if after_crate {
                    self.crate_segments.insert(path.ident.to_string());
                } else if path.ident == "crate" {
                    self.add_use(&path.tree, true);
                } else {
                    self.extern_roots.insert(path.ident.to_string());
                    self.add_use(&path.tree, false);
                }
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use(tree, after_crate);
                }
            }
            UseTree::Name(name) => {
                if after_crate {
                    self.crate_segments.insert(name.ident.to_string());
                }
            }
            UseTree::Rename(rename) => {
                if after_crate {
                    self.crate_segments.insert(rename.ident.to_string());
                }
            }
            UseTree::Glob(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for Refs {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        self.add_use(&node.tree, false);
    }

    fn visit_path(&mut self, node: &'ast SynPath) {
        self.add_path(node);
        visit::visit_path(self, node);
    }
}

fn sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);

    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .expect("a file found below the source root")
                .to_path_buf();
            let text = fs::read_to_string(&path).expect("a source file is readable");
            let parsed = syn::parse_file(&text)
                .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()));
            let mut refs = Refs::default();
            refs.visit_file(&parsed);

            Source {
                layer: layer_of(&relative),
                relative,
                path,
                refs,
            }
        })
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the source tree is readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// The directory a file sits in, when it is not at the source root.
fn layer_of(relative: &Path) -> Option<String> {
    let mut components = relative.components();
    let first = components.next()?.as_os_str().to_str()?.to_owned();
    components.next()?;
    Some(first)
}

/// A file names only the modules its layer allows.
#[test]
fn every_layer_names_only_what_architecture_declares() {
    let mut checked = 0;
    for source in sources() {
        let Some(layer) = source.layer.as_deref() else {
            continue;
        };
        let (_, allowed) = EDGES
            .iter()
            .find(|(name, _)| *name == layer)
            .unwrap_or_else(|| {
                panic!(
                    "{} belongs to the unlisted layer {layer}",
                    source.path.display()
                )
            });

        for segment in &source.refs.crate_segments {
            if segment == layer || segment == "test_support" {
                continue;
            }
            assert!(
                allowed.contains(&segment.as_str()),
                "{} names crate::{segment}, outside {layer}'s edges",
                source.path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "no source file was checked");
}

/// SQL stays in the repositories: the service layer names no sqlx, statement or transaction.
#[test]
fn the_service_layer_names_no_sql() {
    for source in sources() {
        if !source.relative.starts_with("domain/service") {
            continue;
        }
        assert!(
            !source.refs.extern_roots.contains("sqlx"),
            "{} names sqlx; every statement and transaction belongs to a repo",
            source.path.display()
        );
    }
}
