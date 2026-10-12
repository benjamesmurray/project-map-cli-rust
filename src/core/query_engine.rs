use crate::core::graph::{
    EntityMatch, IndexMeta, NodeData, NodeType, ProjectGraph, SnippetPreview,
};
use crate::core::utils::normalize_rel_path;
use crate::error::Result;
use petgraph::visit::Dfs;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub delays: Vec<std::time::Duration>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            delays: vec![
                std::time::Duration::from_millis(100),
                std::time::Duration::from_millis(250),
                std::time::Duration::from_millis(500),
            ],
        }
    }
}

impl RetryPolicy {
    pub fn new(max_retries: usize, delays: Vec<std::time::Duration>) -> Self {
        Self {
            max_retries,
            delays,
        }
    }

    pub fn none() -> Self {
        Self {
            max_retries: 0,
            delays: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct QueryEngine {
    graph: ProjectGraph,
}

impl QueryEngine {
    pub fn load(path: &Path) -> Result<Self> {
        let graph = ProjectGraph::load(path)?;
        Ok(Self { graph })
    }

    pub fn load_with_retry(path: &Path, policy: RetryPolicy) -> Result<Self> {
        let mut attempts = 0;
        loop {
            match Self::load(path) {
                Ok(engine) => return Ok(engine),
                Err(err) => {
                    if attempts >= policy.max_retries {
                        return Err(err);
                    }
                    let delay = policy.delays.get(attempts).copied().unwrap_or_else(|| {
                        policy
                            .delays
                            .last()
                            .copied()
                            .unwrap_or(std::time::Duration::from_millis(100))
                    });
                    std::thread::sleep(delay);
                    attempts += 1;
                }
            }
        }
    }

    pub fn meta(&self) -> Option<&IndexMeta> {
        self.graph.meta.as_ref()
    }

    pub fn find_symbols(&self, query: &str) -> Vec<NodeData> {
        let keywords: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        if keywords.is_empty() {
            return Vec::new();
        }

        self.graph
            .graph
            .node_weights()
            .filter(|n| {
                if n.node_type == NodeType::File {
                    return false;
                }

                let name_lower = n.name.to_lowercase();
                let doc_lower = n
                    .docstring
                    .as_ref()
                    .map(|d| d.to_lowercase())
                    .unwrap_or_default();

                // Match if ALL keywords are found in either name or docstring
                keywords
                    .iter()
                    .all(|k| name_lower.contains(k) || doc_lower.contains(k))
            })
            .cloned()
            .collect()
    }

    pub fn get_file_outline(&self, path: &str) -> Vec<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        if norm_path == "." {
            return self
                .graph
                .graph
                .node_weights()
                .filter(|n| n.node_type == NodeType::File)
                .cloned()
                .collect();
        }

        let file_node = self.graph.graph.node_indices().find(|i| {
            let n = &self.graph.graph[*i];
            n.node_type == NodeType::File
                && (n.path == norm_path || normalize_rel_path(Path::new("."), &n.path) == norm_path)
        });

        if let Some(idx) = file_node {
            use petgraph::visit::EdgeRef;
            self.graph
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
                .map(|edge| {
                    let mut node = self.graph.graph[edge.target()].clone();
                    if *edge.weight() == crate::core::graph::EdgeType::Imports {
                        node.role = Some("Import".to_string());
                    }
                    node
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn analyze_impact(&self, name: &str) -> Vec<NodeData> {
        let node_idx = self
            .graph
            .graph
            .node_indices()
            .find(|i| self.graph.graph[*i].name == name);

        if let Some(start_node) = node_idx {
            // Impact: who do I depend on? (Outgoing edges)
            let mut dfs = Dfs::new(&self.graph.graph, start_node);
            let mut results = Vec::new();
            while let Some(nx) = dfs.next(&self.graph.graph) {
                if nx != start_node {
                    results.push(self.graph.graph[nx].clone());
                }
            }
            results
        } else {
            Vec::new()
        }
    }

    pub fn check_blast_radius(&self, path: &str, symbol: &str) -> Vec<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        let node_idx = self.graph.graph.node_indices().find(|i| {
            let node = &self.graph.graph[*i];
            (node.path == norm_path || normalize_rel_path(Path::new("."), &node.path) == norm_path)
                && node.name == symbol
                && node.node_type != NodeType::File
        });

        let start_node = if let Some(idx) = node_idx {
            idx
        } else {
            // Try matching just by file path if symbol not found
            let file_node = self.graph.graph.node_indices().find(|i| {
                let node = &self.graph.graph[*i];
                (node.path == norm_path
                    || normalize_rel_path(Path::new("."), &node.path) == norm_path)
                    && node.node_type == NodeType::File
            });
            if let Some(idx) = file_node {
                idx
            } else {
                return Vec::new();
            }
        };

        // Blast Radius: who depends on me? (Incoming edges)
        // We need to use a graph traversal that follows edges backwards.
        // petgraph's Dfs follows outgoing edges. To follow incoming, we can use a custom traversal or reverse the graph.
        // Alternatively, we can use neighbors_directed with Incoming in a loop.

        let mut results = Vec::new();
        let mut stack = vec![start_node];
        let mut visited = std::collections::HashSet::new();
        visited.insert(start_node);

        while let Some(current) = stack.pop() {
            for neighbor in self
                .graph
                .graph
                .neighbors_directed(current, petgraph::Direction::Incoming)
            {
                if visited.insert(neighbor) {
                    results.push(self.graph.graph[neighbor].clone());
                    stack.push(neighbor);
                }
            }
        }

        results
    }

    pub fn find_symbol_in_path(&self, path: &str, name: &str) -> Option<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        self.graph
            .graph
            .node_weights()
            .find(|n| {
                n.node_type != NodeType::File
                    && (n.path == norm_path
                        || normalize_rel_path(Path::new("."), &n.path) == norm_path)
                    && n.name == name
            })
            .cloned()
    }

    pub fn find_files(&self, query: &str) -> Vec<NodeData> {
        let query_lower = query.to_lowercase();
        self.graph
            .graph
            .node_weights()
            .filter(|n| {
                n.node_type == NodeType::File && n.path.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect()
    }

    pub fn get_all_file_paths(&self) -> Vec<String> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.node_type == NodeType::File)
            .map(|n| n.path.clone())
            .collect()
    }

    pub fn get_symbol_count(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.node_type != NodeType::File)
            .count()
    }

    pub fn get_file_count(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.node_type == NodeType::File)
            .count()
    }

    pub fn get_edge_count(&self) -> usize {
        self.graph.graph.edge_count()
    }

    pub fn get_last_indexed_time(path: &Path) -> Option<String> {
        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                let datetime: chrono::DateTime<chrono::Utc> = modified.into();
                return Some(datetime.to_rfc3339());
            }
        }
        None
    }

    pub fn find_entities_with_preview(
        &self,
        query: &str,
        preview_lines: Option<usize>,
    ) -> Vec<EntityMatch> {
        let nodes = self.find_symbols(query);
        let mut extractor = SnippetExtractor::new();

        nodes
            .into_iter()
            .map(|n| {
                let preview = preview_lines.and_then(|lines| {
                    if n.line > 0 {
                        extractor.extract(&n.path, n.line, lines)
                    } else {
                        None
                    }
                });

                EntityMatch {
                    name: n.name,
                    kind: n.kind,
                    node_type: n.node_type,
                    path: n.path,
                    line: n.line,
                    role: n.role,
                    preview,
                }
            })
            .collect()
    }
}

pub struct SnippetExtractor {
    file_cache: HashMap<String, Vec<String>>,
}

impl Default for SnippetExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SnippetExtractor {
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
        }
    }

    pub fn extract(
        &mut self,
        path: &str,
        target_line: usize,
        context_lines: usize,
    ) -> Option<SnippetPreview> {
        let lines = if let Some(cached) = self.file_cache.get(path) {
            cached.clone()
        } else {
            let content = std::fs::read_to_string(path).ok()?;
            let read_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            self.file_cache.insert(path.to_string(), read_lines.clone());
            read_lines
        };

        if lines.is_empty() || target_line == 0 {
            return None;
        }

        let total_lines = lines.len();
        let target_idx = target_line.saturating_sub(1);
        if target_idx >= total_lines {
            return None;
        }

        let start_idx = target_idx.saturating_sub(context_lines);
        let end_idx = std::cmp::min(total_lines, target_idx + context_lines + 1);

        let mut snippet_lines = Vec::new();
        let mut formatted = String::new();

        let lang = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("text");

        formatted.push_str(&format!("```{lang}\n"));
        for (idx, line_content) in lines.iter().enumerate().take(end_idx).skip(start_idx) {
            let line_num = idx + 1;
            snippet_lines.push((line_num, line_content.clone()));
            formatted.push_str(&format!("{:4}: {}\n", line_num, line_content));
        }
        formatted.push_str("```");

        Some(SnippetPreview {
            target_line,
            start_line: start_idx + 1,
            end_line: end_idx,
            lines: snippet_lines,
            formatted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snippet_extractor_caching_and_formatting() {
        let temp_path = std::env::temp_dir().join("test_snippet_sample.rs");
        let content = "fn one() {}\nfn two() {\n    println!(\"hello\");\n}\nfn three() {}\n";
        std::fs::write(&temp_path, content).unwrap();

        let mut extractor = SnippetExtractor::new();
        let preview = extractor
            .extract(temp_path.to_str().unwrap(), 3, 1)
            .expect("Failed to extract");

        assert_eq!(preview.target_line, 3);
        assert_eq!(preview.start_line, 2);
        assert_eq!(preview.end_line, 4);
        assert!(preview.formatted.contains("```rs"));
        assert!(preview.formatted.contains("   3:     println!(\"hello\");"));

        // Second extract uses cache
        let preview2 = extractor
            .extract(temp_path.to_str().unwrap(), 1, 0)
            .expect("Failed to extract");
        assert_eq!(preview2.target_line, 1);
        assert_eq!(preview2.start_line, 1);
        assert_eq!(preview2.end_line, 1);

        std::fs::remove_file(&temp_path).ok();
    }
}
