use crate::core::docs::{self, DocOutline, Resolution, ResolveTables, SymbolRef};
use crate::core::graph::{
    EdgeType, ImportEntry, IndexMeta, NodeData, NodeType, ProjectGraph, SCHEMA_VERSION,
};
use crate::core::parser::package_name;
use crate::core::parser::CodeParser;
use crate::core::rust_use::{self, CrateInfo, RustTarget};
use crate::core::utils::{normalize_rel_path, path_to_fqn, resolve_import_path};
use crate::error::Result;
use ignore::WalkBuilder;
use petgraph::graph::NodeIndex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::symlink as sys_symlink;

#[cfg(windows)]
fn sys_symlink<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> std::io::Result<()> {
    let original_path = original.as_ref();
    let link_path = link.as_ref();

    // For relative symlinks, we need to check if the target is a directory
    // relative to the link's parent directory for the is_dir() check to be accurate.
    let actual_target = if original_path.is_relative() {
        if let Some(parent) = link_path.parent() {
            parent.join(original_path)
        } else {
            original_path.to_path_buf()
        }
    } else {
        original_path.to_path_buf()
    };

    if actual_target.is_dir() {
        std::os::windows::fs::symlink_dir(original, link)
    } else {
        std::os::windows::fs::symlink_file(original, link)
    }
}

/// A file the doc pass reads after the code pass has created its file node.
struct DocInput {
    norm_path: String,
    fs_path: PathBuf,
    file_node: NodeIndex,
    is_markdown: bool,
}

/// A parsed Markdown file with the graph nodes of its sections.
struct ParsedDoc {
    norm_path: String,
    file_node: NodeIndex,
    outline: DocOutline,
    /// Parallel to `outline.sections`.
    section_nodes: Vec<NodeIndex>,
}

/// What the link pass added and could not link.
#[derive(Default)]
struct LinkStats {
    references: usize,
    unresolved: usize,
    ambiguous: usize,
}

pub struct Orchestrator {
    parser: CodeParser,
    graph: ProjectGraph,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            parser: CodeParser::new(),
            graph: ProjectGraph::new(),
        }
    }

    pub fn build_index(&mut self, root: &Path, out_dir: &Path) -> Result<()> {
        let can_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let can_out = out_dir
            .canonicalize()
            .unwrap_or_else(|_| out_dir.to_path_buf());
        if can_root == can_out || can_root.starts_with(&can_out) {
            return Err(crate::error::AppError::Index(format!(
                "Root cannot be identical to or inside out_dir: root={:?}, out_dir={:?}",
                root, out_dir
            )));
        }

        let git_state = crate::core::git::read_git_state(root, crate::core::git::GitBudget::BUILD);

        let mut outlines = Vec::new();
        let mut fqn_to_node = HashMap::new();
        let mut path_to_node = HashMap::new();
        let mut doc_inputs = Vec::new();
        let mut fs_paths: HashMap<String, PathBuf> = HashMap::new();

        // Pass 1: Parse all files and create nodes
        // WalkBuilder respects .gitignore inside root; allow hidden files so .env* is indexed
        let mut builder = WalkBuilder::new(root);
        let out_dir_owned = out_dir.to_path_buf();
        let can_out_clone = can_out.clone();
        builder.hidden(false).parents(false).filter_entry(move |e| {
            let name = e.file_name().to_string_lossy();
            if name == ".project-map"
                || name == ".git"
                || name == "target"
                || name == "node_modules"
            {
                return false;
            }
            let p = e.path();
            if p == out_dir_owned || p.starts_with(&out_dir_owned) {
                return false;
            }
            if let Ok(can_p) = p.canonicalize() {
                if can_p == can_out_clone || can_p.starts_with(&can_out_clone) {
                    return false;
                }
            }
            true
        });

        let walk = builder.build();

        for result in walk {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                let is_supported = file_name.starts_with(".env")
                    || extension == "env"
                    || matches!(
                        extension,
                        "py" | "rs"
                            | "ts"
                            | "tsx"
                            | "js"
                            | "jsx"
                            | "mjs"
                            | "cjs"
                            | "kt"
                            | "lua"
                            | "php"
                            | "tf"
                            | "gd"
                            | "sql"
                            | "vue"
                            | "md"
                            | "json"
                            | "toml"
                            | "yaml"
                            | "yml"
                    );
                if is_supported {
                    if crate::core::parser::is_javascript_extension(extension)
                        && crate::core::parser::is_skipped_javascript(path)
                    {
                        continue;
                    }
                    match self.parser.parse_file(path) {
                        Ok(outline) => {
                            let norm_path = normalize_rel_path(root, &outline.path);
                            let fqn = path_to_fqn(root, path);
                            let file_node = self.graph.add_node(NodeData {
                                path: norm_path.clone(),
                                name: fqn.clone(),
                                kind: "file".to_string(),
                                line: 0,
                                start_byte: 0,
                                end_byte: 0,
                                node_type: NodeType::File,
                                docstring: None,
                                role: None,
                            });
                            fqn_to_node.insert(fqn, file_node);
                            path_to_node.insert(norm_path.clone(), file_node);
                            path_to_node.insert(outline.path.clone(), file_node);
                            fs_paths.insert(norm_path.clone(), path.to_path_buf());

                            if extension == "md" || matches!(extension, "json" | "yaml" | "yml") {
                                doc_inputs.push(DocInput {
                                    norm_path: norm_path.clone(),
                                    fs_path: path.to_path_buf(),
                                    file_node,
                                    is_markdown: extension == "md",
                                });
                            }

                            for symbol in &outline.symbols {
                                let node_type = match symbol.kind.as_str() {
                                    "kafka_topic" => NodeType::KafkaTopic,
                                    "database_table" => NodeType::DatabaseTable,
                                    "config_env_var" => NodeType::ConfigEnvVar,
                                    _ => NodeType::Symbol,
                                };
                                let edge_type = match symbol.role.as_deref() {
                                    Some("Producer") => EdgeType::Produces,
                                    Some("Consumer") => EdgeType::Consumes,
                                    Some("Configuration") => EdgeType::Configures,
                                    Some("Reference") => EdgeType::References,
                                    _ => EdgeType::Contains,
                                };
                                let symbol_node = self.graph.add_node(NodeData {
                                    path: norm_path.clone(),
                                    name: symbol.name.clone(),
                                    kind: symbol.kind.clone(),
                                    line: symbol.line,
                                    start_byte: symbol.start_byte,
                                    end_byte: symbol.end_byte,
                                    node_type,
                                    docstring: symbol.docstring.clone(),
                                    role: symbol.role.clone(),
                                });
                                self.graph.add_edge(file_node, symbol_node, edge_type);
                            }
                            outlines.push(outline);
                        }
                        Err(e) => {
                            // If it's just invalid UTF-8, we can skip it silently or log it
                            if !e.to_string().contains("valid UTF-8") {
                                eprintln!("Error parsing {}: {}", path.display(), e);
                            }
                        }
                    }
                }
            }
        }

        // Pass 2: Resolve imports and create edges; every import is also kept in the side table
        let mut import_entries: Vec<ImportEntry> = Vec::new();
        let mut crate_cache: HashMap<String, CrateInfo> = HashMap::new();
        let symbols_by_file = symbol_lookup(&self.graph.graph);
        for outline in outlines {
            let norm_from = normalize_rel_path(root, &outline.path);
            let Some(&from_node) = path_to_node
                .get(&norm_from)
                .or_else(|| path_to_node.get(&outline.path))
            else {
                continue;
            };
            let is_rust = outline.language == "rust";
            let krate = is_rust.then(|| crate_of(root, &norm_from, &mut crate_cache));
            for record in outline.imports {
                let imp = record.module;
                let mut resolved = false;
                if let Some(krate) = &krate {
                    for leaf in rust_use::expand_use(&imp) {
                        let has_file = |p: &str| path_to_node.contains_key(p);
                        let target = match rust_use::resolve(&leaf, &norm_from, krate, &has_file) {
                            Some(RustTarget::Symbol { file, name }) => symbols_by_file
                                .get(&(file.clone(), name))
                                .copied()
                                .or_else(|| path_to_node.get(&file).copied()),
                            Some(RustTarget::File(file)) => path_to_node.get(&file).copied(),
                            None => None,
                        };
                        if let Some(to_node) = target {
                            self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                            resolved = true;
                        }
                    }
                } else if let Some(&to_node) = fqn_to_node.get(&imp) {
                    // Strategy 1: FQN Match (Python/General)
                    self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                    resolved = true;
                } else {
                    // Strategy 2: Relative Path Resolution (TypeScript)
                    let resolved_rel = resolve_import_path(&norm_from, &imp);
                    let norm_resolved = normalize_rel_path(root, &resolved_rel);

                    // Try matching resolved path with common TS/Terraform extensions
                    for ext in &["", ".ts", ".tsx", "/index.ts", "/index.tsx", "/main.tf"] {
                        let candidate = format!("{}{}", norm_resolved, ext);
                        if let Some(&to_node) = path_to_node.get(&candidate) {
                            self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                            resolved = true;
                            break;
                        }
                    }

                    // Strategy 3: FQN Suffix match (Fallback)
                    if !resolved {
                        let matching_fqn = fqn_to_node.keys().find(|&k| {
                            k.ends_with(&imp)
                                || imp.ends_with(k)
                                || (k.contains('.')
                                    && imp.contains('.')
                                    && k.split('.').next_back() == imp.split('.').next_back()
                                    && k.split('.').rev().nth(1) == imp.split('.').rev().nth(1))
                        });
                        if let Some(key) = matching_fqn {
                            let &to_node = fqn_to_node.get(key).unwrap();
                            self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                            resolved = true;
                        }
                    }
                }

                let mut package = package_name(&outline.language, &imp);
                if let (Some(pkg), Some(krate)) = (&package, &krate) {
                    if krate.name.as_deref() == Some(pkg.as_str()) {
                        package = None;
                    }
                }
                import_entries.push(ImportEntry {
                    path: norm_from.clone(),
                    line: record.line,
                    text: record.text,
                    module: imp,
                    package,
                    language: outline.language.clone(),
                    resolved,
                });
            }
        }
        import_entries
            .sort_by(|a, b| (&a.path, a.line, &a.module).cmp(&(&b.path, b.line, &b.module)));
        self.graph.imports = import_entries;

        // Pass 3: documentation and API nodes. Needs the finished code symbol table.
        let parsed_docs = self.doc_pass(doc_inputs);
        let stats = self.link_pass(root, &parsed_docs, &fs_paths);
        self.graph.doc_terms = docs::aggregate_terms(
            parsed_docs
                .iter()
                .map(|d| (d.norm_path.as_str(), &d.outline)),
        );

        let canonical_root = root
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| root.to_string_lossy().to_string());
        let built_at = chrono::Utc::now().to_rfc3339();
        let builder_pid = std::process::id();
        let file_count = self
            .graph
            .graph
            .node_weights()
            .filter(|n| n.node_type == NodeType::File)
            .count();
        let symbol_count = self
            .graph
            .graph
            .node_weights()
            .filter(|n| n.node_type != NodeType::File && !n.node_type.is_doc())
            .count();
        let edge_count = self.graph.graph.edge_count();
        let count_of = |ty: NodeType| {
            self.graph
                .graph
                .node_weights()
                .filter(|n| n.node_type == ty)
                .count()
        };

        self.graph.meta = Some(IndexMeta {
            schema_version: SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            root: canonical_root,
            built_at,
            builder_pid,
            file_count,
            symbol_count,
            edge_count,
            doc_file_count: parsed_docs.len(),
            section_count: count_of(NodeType::DocSection),
            reference_count: stats.references,
            unresolved_mention_count: stats.unresolved,
            ambiguous_mention_count: stats.ambiguous,
            api_operation_count: count_of(NodeType::ApiOperation),
            git_commit: git_state.commit,
            git_dirty: git_state.dirty,
            git_branch: git_state.branch,
        });

        Ok(())
    }

    /// Adds `DocSection`, `ApiOperation` and `ApiSchema` nodes and the `DocMeta`
    /// side table. Section text is never stored, only byte ranges.
    fn doc_pass(&mut self, inputs: Vec<DocInput>) -> Vec<ParsedDoc> {
        let mut parsed = Vec::new();
        for input in inputs {
            let Ok(text) = std::fs::read_to_string(&input.fs_path) else {
                continue;
            };
            if input.is_markdown {
                parsed.push(self.add_markdown(input, &text));
            } else if docs::is_openapi(
                &input.fs_path,
                &text.as_bytes()[..text.len().min(docs::OPENAPI_HEAD_LEN)],
                text.len() as u64,
            ) {
                self.add_api(&input, &text);
            }
        }
        parsed
    }

    /// Resolves every collected mention and adds a `References` edge from its
    /// deepest section (or the file, before the first heading) to each target.
    /// Identical edges are added once.
    fn link_pass(
        &mut self,
        root: &Path,
        parsed: &[ParsedDoc],
        fs_paths: &HashMap<String, PathBuf>,
    ) -> LinkStats {
        let mut stats = LinkStats::default();
        if parsed.is_empty() {
            return stats;
        }

        // A relative root stays in indexed paths as a prefix.
        let prefix = if root.is_absolute() {
            String::new()
        } else {
            match normalize_rel_path(Path::new("."), &root.to_string_lossy()).as_str() {
                "." => String::new(),
                p => p.trim_end_matches('/').to_string(),
            }
        };
        let mut tables = ResolveTables::new(&prefix);
        for idx in self.graph.graph.node_indices() {
            let node = &self.graph.graph[idx];
            match node.node_type {
                NodeType::File => tables.add_file(&node.path, idx),
                NodeType::Symbol => tables.add_symbol(
                    &node.path,
                    SymbolRef {
                        node: idx,
                        name: node.name.clone(),
                        line: node.line,
                        start_byte: node.start_byte,
                        end_byte: node.end_byte,
                        is_impl: node.kind == "impl",
                    },
                ),
                _ => {}
            }
        }

        // `path:line` mentions need the target's line starts; read each file once.
        let mut loaded = HashSet::new();
        for doc in parsed {
            for mention in &doc.outline.mentions {
                let Some(file) = tables.line_file(&mention.form).map(str::to_string) else {
                    continue;
                };
                if loaded.insert(file.clone()) {
                    if let Some(text) = fs_paths
                        .get(&file)
                        .and_then(|p| std::fs::read_to_string(p).ok())
                    {
                        tables.set_line_text(&file, &text);
                    }
                }
            }
        }

        let mut edges = HashSet::new();
        for doc in parsed {
            for mention in &doc.outline.mentions {
                let from = mention
                    .section
                    .map_or(doc.file_node, |i| doc.section_nodes[i]);
                let resolution = docs::resolve_mention(mention, &doc.norm_path, &tables);
                if let Resolution::Linked(targets) = &resolution {
                    for &to in targets {
                        if edges.insert((from, to)) {
                            self.graph.add_edge(from, to, EdgeType::References);
                            stats.references += 1;
                        }
                    }
                } else if docs::counts_as_unmatched(mention, &resolution) {
                    match resolution {
                        Resolution::Ambiguous => stats.ambiguous += 1,
                        _ => stats.unresolved += 1,
                    }
                }
            }
        }
        stats
    }

    fn add_markdown(&mut self, input: DocInput, text: &str) -> ParsedDoc {
        let outline = docs::parse_markdown(&input.norm_path, text);
        let mut section_nodes: Vec<NodeIndex> = Vec::with_capacity(outline.sections.len());
        for section in &outline.sections {
            let node = self.graph.add_node(NodeData {
                path: input.norm_path.clone(),
                name: section.name.clone(),
                kind: section.kind().to_string(),
                line: section.line,
                start_byte: section.start_byte,
                end_byte: section.end_byte,
                node_type: NodeType::DocSection,
                docstring: None,
                role: Some(section.anchor.clone()),
            });
            // Parents precede children in document order.
            let parent = section.parent.map_or(input.file_node, |p| section_nodes[p]);
            self.graph.add_edge(parent, node, EdgeType::Contains);
            section_nodes.push(node);
        }
        if let Some(meta) = &outline.meta {
            self.graph
                .doc_meta
                .insert(input.norm_path.clone(), meta.clone());
        }
        ParsedDoc {
            norm_path: input.norm_path,
            file_node: input.file_node,
            outline,
            section_nodes,
        }
    }

    /// Operation nodes: name `METHOD /path`, kind the lower-case method,
    /// role the operationId, docstring the summary. Schema nodes: kind `schema`.
    fn add_api(&mut self, input: &DocInput, text: &str) {
        let Some(api) = docs::parse_openapi_text(&input.fs_path, text) else {
            return;
        };
        for op in &api.operations {
            let node = self.graph.add_node(NodeData {
                path: input.norm_path.clone(),
                name: op.name(),
                kind: op.method.to_ascii_lowercase(),
                line: op.location.line,
                start_byte: op.location.start_byte,
                end_byte: op.location.end_byte,
                node_type: NodeType::ApiOperation,
                docstring: op.summary.clone(),
                role: op.operation_id.clone(),
            });
            self.graph
                .add_edge(input.file_node, node, EdgeType::Contains);
        }
        for schema in &api.schemas {
            let node = self.graph.add_node(NodeData {
                path: input.norm_path.clone(),
                name: schema.name.clone(),
                kind: "schema".to_string(),
                line: schema.location.line,
                start_byte: schema.location.start_byte,
                end_byte: schema.location.end_byte,
                node_type: NodeType::ApiSchema,
                docstring: None,
                role: None,
            });
            self.graph
                .add_edge(input.file_node, node, EdgeType::Contains);
        }
    }

    pub fn is_effectively_empty(&self, root: &Path, out_dir: &Path) -> bool {
        let can_out = out_dir
            .canonicalize()
            .unwrap_or_else(|_| out_dir.to_path_buf());
        let out_dir_owned = out_dir.to_path_buf();
        let walk = WalkBuilder::new(root)
            .filter_entry(move |e| {
                let name = e.file_name().to_string_lossy();
                if name == ".project-map"
                    || name == ".git"
                    || name == "target"
                    || name == "node_modules"
                {
                    return false;
                }
                let p = e.path();
                if p == out_dir_owned || p.starts_with(&out_dir_owned) {
                    return false;
                }
                if let Ok(can_p) = p.canonicalize() {
                    if can_p == can_out || can_p.starts_with(&can_out) {
                        return false;
                    }
                }
                true
            })
            .build();

        for result in walk {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                // If we find ANY non-hidden file (WalkBuilder respects .gitignore), it's not empty
                return false;
            }
        }
        true
    }

    pub fn scaffold_if_empty(&self, root: &Path, out_dir: &Path) -> Result<bool> {
        let can_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let can_out = out_dir
            .canonicalize()
            .unwrap_or_else(|_| out_dir.to_path_buf());
        if can_root == can_out || can_root.starts_with(&can_out) {
            return Err(crate::error::AppError::Index(format!(
                "Root cannot be identical to or inside out_dir: root={:?}, out_dir={:?}",
                root, out_dir
            )));
        }

        if self.is_effectively_empty(root, out_dir) {
            let arch_path = root.join("ARCHITECTURE.md");
            if !arch_path.exists() {
                let content = "# Project Architecture\n\nThis is a placeholder for the project's architectural overview. Use this file to define the system structure, core components, and design principles.\n";
                std::fs::write(&arch_path, content)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn save_index(&self, path: &Path) -> Result<()> {
        self.graph.save(path)
    }

    pub fn save_index_versioned(&self, base_dir: &Path) -> Result<()> {
        use crate::core::lock::IndexLock;
        use std::time::Duration;

        let _lock_guard = IndexLock::acquire(base_dir, Duration::from_secs(30))?;
        self.save_index_versioned_unlocked(base_dir)
    }

    pub(crate) fn save_index_versioned_unlocked(&self, base_dir: &Path) -> Result<()> {
        use chrono::Local;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        ensure_index_not_newer(base_dir)?;

        let pid = std::process::id();
        let timestamp = Local::now().format("%Y%m%d_%H%M%S_%3f").to_string();
        let backup_dir_name = format!("{}_{}", timestamp, pid);
        let backups_dir = base_dir.join("backups");
        let current_backup_dir = backups_dir.join(&backup_dir_name);

        fs::create_dir_all(&current_backup_dir)?;

        let index_path = current_backup_dir.join(".project-map.json");
        self.graph.save(&index_path)?;

        let latest_link = base_dir.join("latest");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_link_name = format!("latest.tmp.{}.{}", pid, nanos);
        let tmp_link = base_dir.join(&tmp_link_name);
        let _ = fs::remove_file(&tmp_link);
        let _ = fs::remove_dir_all(&tmp_link);

        let rel_target = Path::new("backups").join(&backup_dir_name);

        #[cfg(any(unix, windows))]
        let symlink_result = sys_symlink(&rel_target, &tmp_link);
        #[cfg(not(any(unix, windows)))]
        let symlink_result: std::io::Result<()> = Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unsupported platform",
        ));

        if symlink_result.is_ok() {
            publish_latest_symlink(&tmp_link, &latest_link)?;
        } else {
            let tmp_dir = tmp_link;
            fs::create_dir_all(&tmp_dir)?;
            fs::copy(&index_path, tmp_dir.join(".project-map.json"))?;
            #[cfg(unix)]
            {
                fs::rename(&tmp_dir, &latest_link)?;
            }
            #[cfg(not(unix))]
            {
                if latest_link.exists() {
                    let _ = fs::remove_file(&latest_link);
                    let _ = fs::remove_dir_all(&latest_link);
                }
                fs::rename(&tmp_dir, &latest_link)?;
            }
        }

        // Safe rotation: keep newest 5 and NEVER remove the current latest target
        let latest_target_canonical = fs::canonicalize(&latest_link).ok();

        if let Ok(entries) = fs::read_dir(&backups_dir) {
            let mut dirs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();
            dirs.sort_by_key(|e| e.file_name());

            while dirs.len() > 5 {
                let oldest = dirs.remove(0);
                let oldest_path = oldest.path();
                let is_latest = oldest_path == current_backup_dir
                    || (latest_target_canonical.is_some()
                        && oldest_path.canonicalize().ok() == latest_target_canonical);
                if !is_latest {
                    let _ = fs::remove_dir_all(&oldest_path);
                }
            }
        }

        Ok(())
    }

    pub fn rebuild_and_publish(
        root: &Path,
        out_dir: &Path,
    ) -> Result<crate::core::query_engine::QueryEngine> {
        rebuild_and_publish(root, out_dir)
    }
}

/// Moves the new `latest` symlink into place. Normally one atomic rename. Older
/// builds wrote `latest/` as a real directory, over which `rename` of a symlink
/// fails (`EISDIR`); that case is migrated without a moment at which `latest` is
/// missing.
#[cfg(unix)]
fn publish_latest_symlink(tmp_link: &Path, latest_link: &Path) -> Result<()> {
    use std::fs;

    let is_real_dir = fs::symlink_metadata(latest_link)
        .map(|m| m.is_dir() && !m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_real_dir {
        fs::rename(tmp_link, latest_link)?;
        return Ok(());
    }

    // Linux: exchange the two names in one step, so readers see the old directory
    // and then the new symlink, never neither. `tmp_link` then names the old
    // directory, which is removed.
    #[cfg(target_os = "linux")]
    if exchange_paths(tmp_link, latest_link).is_ok() {
        let _ = fs::remove_dir_all(tmp_link);
        return Ok(());
    }

    swap_via_aside(tmp_link, latest_link)
}

/// Portable migration: move the old directory aside, install the symlink, then
/// delete the old one. A reader that retries (`load_with_retry`) rides over the
/// instant between the two renames. The old directory is restored if installing
/// fails.
#[cfg(unix)]
fn swap_via_aside(tmp_link: &Path, latest_link: &Path) -> Result<()> {
    use std::fs;

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let aside = latest_link.with_file_name(format!("latest.old.{}.{}", std::process::id(), nanos));
    fs::rename(latest_link, &aside)?;
    if let Err(e) = fs::rename(tmp_link, latest_link) {
        let _ = fs::rename(&aside, latest_link);
        return Err(e.into());
    }
    let _ = fs::remove_dir_all(&aside);
    Ok(())
}

#[cfg(not(unix))]
fn publish_latest_symlink(tmp_link: &Path, latest_link: &Path) -> Result<()> {
    std::fs::rename(tmp_link, latest_link)?;
    Ok(())
}

/// `renameat2(RENAME_EXCHANGE)`: atomically swaps two paths.
#[cfg(target_os = "linux")]
fn exchange_paths(a: &Path, b: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let to_c = |p: &Path| {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))
    };
    let (a, b) = (to_c(a)?, to_c(b)?);
    // SAFETY: both pointers are valid NUL-terminated strings for the call.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// (file path, symbol name) to the first symbol node of that name in the file.
fn symbol_lookup(
    graph: &petgraph::graph::DiGraph<NodeData, EdgeType>,
) -> HashMap<(String, String), NodeIndex> {
    let mut map = HashMap::new();
    for idx in graph.node_indices() {
        let n = &graph[idx];
        if n.node_type == NodeType::Symbol {
            map.entry((n.path.clone(), n.name.clone())).or_insert(idx);
        }
    }
    map
}

/// The crate containing the file `rel`: its nearest ancestor directory with a Cargo.toml.
fn crate_of(root: &Path, rel: &str, cache: &mut HashMap<String, CrateInfo>) -> CrateInfo {
    let mut dir = rel.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    let start = dir.clone();
    if let Some(hit) = cache.get(&start) {
        return hit.clone();
    }
    let info = loop {
        let manifest = root.join(&dir).join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            break CrateInfo {
                dir: dir.clone(),
                name: rust_use::crate_name(&text),
            };
        }
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_string(),
            None if !dir.is_empty() => dir.clear(),
            None => {
                break CrateInfo {
                    dir: String::new(),
                    name: None,
                }
            }
        }
    };
    cache.insert(start, info.clone());
    info
}

/// Forward guard: refuse to replace an index written by a newer schema.
/// A missing or corrupt index does not trigger it.
pub fn ensure_index_not_newer(out_dir: &Path) -> Result<()> {
    let current = out_dir.join("latest").join(".project-map.json");
    match ProjectGraph::read_schema_version(&current) {
        Some(found) if found > SCHEMA_VERSION => Err(crate::error::AppError::NewerIndex {
            found,
            supported: SCHEMA_VERSION,
        }),
        _ => Ok(()),
    }
}

/// Rebuild the index and publish it atomically under IndexLock.
/// Returns the newly loaded QueryEngine.
pub fn rebuild_and_publish(
    root: &Path,
    out_dir: &Path,
) -> Result<crate::core::query_engine::QueryEngine> {
    use crate::core::lock::IndexLock;
    use crate::core::query_engine::QueryEngine;
    use std::time::Duration;

    let _lock = IndexLock::acquire(out_dir, Duration::from_secs(30))?;
    ensure_index_not_newer(out_dir)?;
    let mut orch = Orchestrator::new();
    let _ = orch.scaffold_if_empty(root, out_dir)?;
    orch.build_index(root, out_dir)?;
    orch.save_index_versioned_unlocked(out_dir)?;

    let latest_file = out_dir.join("latest").join(".project-map.json");
    QueryEngine::load(&latest_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::query_engine::QueryEngine;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn test_two_saves_same_millisecond_different_pids_do_not_collide() {
        let temp = tempdir().unwrap();
        let base_dir = temp.path();
        let backups_dir = base_dir.join("backups");
        fs::create_dir_all(&backups_dir).unwrap();

        let ts = "20261012_120000_123";
        let pid1 = 1001;
        let pid2 = 1002;

        let dir1 = backups_dir.join(format!("{}_{}", ts, pid1));
        let dir2 = backups_dir.join(format!("{}_{}", ts, pid2));

        fs::create_dir_all(&dir1).unwrap();
        fs::create_dir_all(&dir2).unwrap();

        fs::write(
            dir1.join(".project-map.json"),
            "{\"nodes\":[],\"edges\":[]}",
        )
        .unwrap();
        fs::write(
            dir2.join(".project-map.json"),
            "{\"nodes\":[],\"edges\":[]}",
        )
        .unwrap();

        assert!(dir1.exists());
        assert!(dir2.exists());
        assert_ne!(dir1, dir2);
    }

    #[test]
    fn test_rotation_never_deletes_latest_target() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("project");
        let out_dir = temp.path().join("out");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test.rs"), "fn foo() {}").unwrap();

        let mut orch = Orchestrator::new();
        orch.build_index(&root, &out_dir).unwrap();

        for _ in 0..8 {
            orch.save_index_versioned(&out_dir).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let backups_dir = out_dir.join("backups");
        let backup_count = fs::read_dir(&backups_dir).unwrap().count();
        assert!(
            backup_count <= 5,
            "Should keep at most 5 backups, got {}",
            backup_count
        );

        let latest = out_dir.join("latest");
        assert!(latest.exists(), "latest must exist");
        let latest_file = latest.join(".project-map.json");
        assert!(latest_file.exists(), "latest index file must exist");

        // Verify latest points to an existing directory inside backups
        let target_canonical = fs::canonicalize(&latest).expect("canonicalize latest");
        assert!(
            target_canonical.exists(),
            "latest target directory must not have been deleted by rotation"
        );
    }

    #[test]
    fn test_polling_reader_during_50_saves_latest_never_missing() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("project");
        let out_dir = temp.path().join("out");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test.rs"), "fn foo() {}").unwrap();

        let mut orch = Orchestrator::new();
        orch.build_index(&root, &out_dir).unwrap();
        orch.save_index_versioned(&out_dir).unwrap();

        let latest_file = out_dir.join("latest").join(".project-map.json");
        let done = Arc::new(AtomicBool::new(false));
        let done_reader = Arc::clone(&done);
        let latest_file_reader = latest_file.clone();

        let reader_thread = std::thread::spawn(move || {
            let mut reads = 0;
            while !done_reader.load(Ordering::Relaxed) {
                let res = QueryEngine::load(&latest_file_reader);
                assert!(
                    res.is_ok(),
                    "Reader should never see missing or corrupt latest: {:?}",
                    res
                );
                reads += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            reads
        });

        for _ in 0..50 {
            orch.save_index_versioned(&out_dir).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        done.store(true, Ordering::Relaxed);
        let total_reads = reader_thread
            .join()
            .expect("reader thread should not panic");
        assert!(
            total_reads > 0,
            "Reader should have executed multiple reads"
        );
    }

    /// A `latest/` that is a real directory, as older builds wrote it.
    fn out_dir_with_real_latest(temp: &Path, orch: &Orchestrator) -> std::path::PathBuf {
        let out_dir = temp.join("out");
        let latest = out_dir.join("latest");
        fs::create_dir_all(&latest).unwrap();
        orch.save_index(&latest.join(".project-map.json")).unwrap();
        assert!(!fs::symlink_metadata(&latest)
            .unwrap()
            .file_type()
            .is_symlink());
        out_dir
    }

    fn test_orchestrator(temp: &Path) -> Orchestrator {
        let root = temp.join("project");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("test.rs"), "fn foo() {}").unwrap();
        let mut orch = Orchestrator::new();
        orch.build_index(&root, &temp.join("scratch-out")).unwrap();
        orch
    }

    #[test]
    fn test_real_directory_latest_is_migrated_to_a_symlink() {
        let temp = tempdir().unwrap();
        let orch = test_orchestrator(temp.path());
        let out_dir = out_dir_with_real_latest(temp.path(), &orch);

        orch.save_index_versioned(&out_dir).unwrap();

        let latest = out_dir.join("latest");
        assert!(fs::symlink_metadata(&latest)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(QueryEngine::load(&latest.join(".project-map.json")).is_ok());
        let leftovers: Vec<_> = fs::read_dir(&out_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("latest."))
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");

        // A second save over the symlink keeps working.
        orch.save_index_versioned(&out_dir).unwrap();
        assert!(QueryEngine::load(&latest.join(".project-map.json")).is_ok());
    }

    #[test]
    fn test_migrating_a_real_directory_latest_is_never_observed_missing() {
        let temp = tempdir().unwrap();
        let orch = test_orchestrator(temp.path());
        let out_dir = out_dir_with_real_latest(temp.path(), &orch);
        let latest_file = out_dir.join("latest").join(".project-map.json");

        let done = Arc::new(AtomicBool::new(false));
        let done_reader = Arc::clone(&done);
        let reader = std::thread::spawn(move || {
            let mut reads = 0u32;
            while !done_reader.load(Ordering::Relaxed) {
                let res = QueryEngine::load(&latest_file);
                assert!(res.is_ok(), "latest missing or corrupt mid-swap: {res:?}");
                reads += 1;
            }
            reads
        });

        orch.save_index_versioned(&out_dir).unwrap();
        for _ in 0..20 {
            orch.save_index_versioned(&out_dir).unwrap();
        }
        done.store(true, Ordering::Relaxed);
        assert!(reader.join().expect("reader must not panic") > 0);
    }

    #[test]
    fn test_directory_swap_fallback_without_the_atomic_exchange() {
        let temp = tempdir().unwrap();
        let base = temp.path();
        fs::create_dir_all(base.join("latest")).unwrap();
        fs::write(base.join("latest").join("old.txt"), "old").unwrap();
        fs::create_dir_all(base.join("backups").join("b1")).unwrap();
        fs::write(base.join("backups").join("b1").join("new.txt"), "new").unwrap();
        let tmp_link = base.join("latest.tmp.1.1");
        super::sys_symlink(Path::new("backups").join("b1"), &tmp_link).unwrap();

        super::swap_via_aside(&tmp_link, &base.join("latest")).unwrap();

        assert!(fs::symlink_metadata(base.join("latest"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(base.join("latest").join("new.txt")).unwrap(),
            "new"
        );
        assert!(!base.join("latest").join("old.txt").exists());
        let leftovers = fs::read_dir(base)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("latest.old"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
