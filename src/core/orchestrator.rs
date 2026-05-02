use std::path::Path;
use std::collections::HashMap;
use ignore::WalkBuilder;
use crate::core::parser::CodeParser;
use crate::core::graph::{ProjectGraph, NodeData, NodeType, EdgeType};
use crate::core::utils::{path_to_fqn, resolve_import_path};
use crate::error::Result;

pub struct Orchestrator {
    parser: CodeParser,
    graph: ProjectGraph,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            parser: CodeParser::new(),
            graph: ProjectGraph::new(),
        }
    }

    pub fn build_index(&mut self, root: &Path) -> Result<()> {
        let mut outlines = Vec::new();
        let mut fqn_to_node = HashMap::new();
        let mut path_to_node = HashMap::new();

        // Pass 1: Parse all files and create nodes
        // WalkBuilder respects .gitignore by default
        let walk = WalkBuilder::new(root)
            .filter_entry(|e| e.file_name() != ".project-map")
            .build();

        for result in walk {
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if extension == "py" || extension == "rs" || extension == "ts" || extension == "tsx" || extension == "kt" || extension == "sql" || extension == "vue" {
                    match self.parser.parse_file(path) {
                        Ok(outline) => {
                            let fqn = path_to_fqn(root, path);
                            let file_node = self.graph.add_node(NodeData {
                                path: outline.path.clone(),
                                name: fqn.clone(),
                                kind: "file".to_string(),
                                line: 0,
                                start_byte: 0,
                                end_byte: 0,
                                node_type: NodeType::File,
                            });
                            fqn_to_node.insert(fqn, file_node);
                            path_to_node.insert(outline.path.clone(), file_node);

                            for symbol in &outline.symbols {
                                let symbol_node = self.graph.add_node(NodeData {
                                    path: outline.path.clone(),
                                    name: symbol.name.clone(),
                                    kind: symbol.kind.clone(),
                                    line: symbol.line,
                                    start_byte: symbol.start_byte,
                                    end_byte: symbol.end_byte,
                                    node_type: NodeType::Symbol,
                                });
                                self.graph.add_edge(file_node, symbol_node, EdgeType::Contains);
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
            if let Some(&from_node) = path_to_node.get(&outline.path) {
                for imp in outline.imports {
                    // Strategy 1: FQN Match (Python/General)
                    if let Some(&to_node) = fqn_to_node.get(&imp) {
                        self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                    } else {
                        // Strategy 2: Relative Path Resolution (TypeScript)
                        let resolved_rel = resolve_import_path(&outline.path, &imp);
                        
                        // Try matching resolved path with common TS extensions
                        let mut found = false;
                        for ext in &["", ".ts", ".tsx", "/index.ts", "/index.tsx"] {
                            let candidate = format!("{}{}", resolved_rel, ext);
                            if let Some(&to_node) = path_to_node.get(&candidate) {
                                self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                                found = true;
                                break;
                            }
                        }

                        // Strategy 3: FQN Suffix match (Fallback)
                        if !found {
                            let matching_fqn = fqn_to_node.keys()
                                .find(|&k| k.ends_with(&imp));
                            if let Some(key) = matching_fqn {
                                let &to_node = fqn_to_node.get(key).unwrap();
                                self.graph.add_edge(from_node, to_node, EdgeType::Imports);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn save_index(&self, path: &Path) -> Result<()> {
        self.graph.save(path)
    }

    pub fn save_index_versioned(&self, base_dir: &Path) -> Result<()> {
        use chrono::Local;
        use std::fs;
        use std::os::unix::fs::symlink;

        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let backups_dir = base_dir.join("backups");
        let current_backup_dir = backups_dir.join(&timestamp);
        
        fs::create_dir_all(&current_backup_dir)?;
        
        let index_path = current_backup_dir.join(".project-map.json");
        self.graph.save(&index_path)?;

        // Limit backups to 5
        if let Ok(entries) = fs::read_dir(&backups_dir) {
            let mut dirs: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            dirs.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH));
            
            while dirs.len() > 5 {
                let oldest = dirs.remove(0);
                fs::remove_dir_all(oldest.path()).ok();
            }
        }

        let latest_link = base_dir.join("latest");
        if latest_link.exists() {
            fs::remove_file(&latest_link).ok();
            fs::remove_dir_all(&latest_link).ok();
        }

        // On Unix, use a symlink. 
        #[cfg(unix)]
        {
            // We want the symlink to be relative so it's portable
            let rel_target = Path::new("backups").join(&timestamp);
            symlink(rel_target, &latest_link)?;
        }

        // Fallback for non-Unix or if symlink fails
        #[cfg(not(unix))]
        {
            fs::create_dir_all(&latest_link)?;
            fs::copy(&index_path, latest_link.join(".project-map.json"))?;
        }

        Ok(())
    }
}
