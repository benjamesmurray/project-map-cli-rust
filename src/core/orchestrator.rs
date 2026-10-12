use crate::core::graph::{EdgeType, IndexMeta, NodeData, NodeType, ProjectGraph};
use crate::core::parser::CodeParser;
use crate::core::utils::{normalize_rel_path, path_to_fqn, resolve_import_path};
use crate::error::Result;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::Path;

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

        let mut outlines = Vec::new();
        let mut fqn_to_node = HashMap::new();
        let mut path_to_node = HashMap::new();

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

        // Pass 2: Resolve imports and create edges
        for outline in outlines {
            let norm_from = normalize_rel_path(root, &outline.path);
            if let Some(&from_node) = path_to_node
                .get(&norm_from)
                .or_else(|| path_to_node.get(&outline.path))
            {
                for imp in outline.imports {
                    // Strategy 1: FQN Match (Python/General)
                    if let Some(&to_node) = fqn_to_node.get(&imp) {
                        self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                    } else {
                        // Strategy 2: Relative Path Resolution (TypeScript)
                        let resolved_rel = resolve_import_path(&norm_from, &imp);
                        let norm_resolved = normalize_rel_path(root, &resolved_rel);

                        // Try matching resolved path with common TS/Terraform extensions
                        let mut found = false;
                        for ext in &["", ".ts", ".tsx", "/index.ts", "/index.tsx", "/main.tf"] {
                            let candidate = format!("{}{}", norm_resolved, ext);
                            if let Some(&to_node) = path_to_node.get(&candidate) {
                                self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                                found = true;
                                break;
                            }
                        }

                        // Strategy 3: FQN Suffix match (Fallback)
                        if !found {
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
                            }
                        }
                    }
                }
            }
        }

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
            .filter(|n| n.node_type != NodeType::File)
            .count();
        let edge_count = self.graph.graph.edge_count();

        self.graph.meta = Some(IndexMeta {
            schema_version: 2,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            root: canonical_root,
            built_at,
            builder_pid,
            file_count,
            symbol_count,
            edge_count,
        });

        Ok(())
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
            fs::rename(&tmp_link, &latest_link)?;
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
}
