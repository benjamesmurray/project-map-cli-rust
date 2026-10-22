use crate::core::graph::{
    EdgeType, EntityMatch, IndexMeta, NodeData, NodeType, ProjectGraph, SnippetPreview,
};
use crate::core::results::BlastMatch;
use crate::core::utils::normalize_rel_path;
use crate::error::Result;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, VecDeque};
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

/// A blast radius walk: the node it started from and its dependents.
pub(crate) struct BlastWalk {
    pub target: Option<(NodeIndex, BlastMatch)>,
    /// For a symbol nested in another (a method), the outermost enclosing symbol the
    /// walk also started from.
    pub via: Option<NodeIndex>,
    /// (depth, node); depth is 1 for a direct dependent. Ordered by depth, path, line, name.
    pub dependents: Vec<(usize, NodeIndex)>,
}

#[derive(Debug)]
pub struct QueryEngine {
    pub(crate) graph: ProjectGraph,
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

    pub fn is_stale(&self) -> bool {
        self.graph.is_stale()
    }

    pub fn lacks_docs(&self) -> bool {
        self.graph.lacks_docs()
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

        let mut found: Vec<NodeData> = self
            .graph
            .graph
            .node_weights()
            .filter(|n| {
                // `search` is code-only; documentation has its own tools.
                if n.node_type == NodeType::File || n.node_type.is_doc() {
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
            .collect();
        found.sort_by(symbol_order);
        found
    }

    pub fn get_file_outline(&self, path: &str) -> Vec<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        if norm_path == "." {
            return self.find_files("");
        }

        if let Some(idx) = self.outline_file_node(&norm_path) {
            let mut outline: Vec<NodeData> = self
                .graph
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
                // Doc -> code references belong to `doc_links`, not to an outline.
                .filter(|edge| *edge.weight() != EdgeType::References)
                .map(|edge| {
                    let mut node = self.graph.graph[edge.target()].clone();
                    if *edge.weight() == EdgeType::Imports {
                        node.role = Some("Import".to_string());
                    }
                    node
                })
                .collect();
            // Imports first (by the imported path, line, name), then the file's own
            // symbols in source order.
            outline.sort_by(|a, b| {
                let (a_import, b_import) = (
                    a.role.as_deref() == Some("Import"),
                    b.role.as_deref() == Some("Import"),
                );
                b_import
                    .cmp(&a_import)
                    .then_with(|| symbol_order_path_first(a, b, a_import))
            });
            outline
        } else {
            Vec::new()
        }
    }

    /// The file node an outline is taken from; `norm_path` is already normalised.
    pub(crate) fn outline_file_node(&self, norm_path: &str) -> Option<NodeIndex> {
        self.graph.graph.node_indices().find(|i| {
            let n = &self.graph.graph[*i];
            n.node_type == NodeType::File
                && (n.path == norm_path || normalize_rel_path(Path::new("."), &n.path) == norm_path)
        })
    }

    /// The node `plan` starts from: of the non-doc nodes with this name, the first by
    /// path, then line.
    pub(crate) fn plan_target(&self, name: &str) -> Option<NodeIndex> {
        self.first_by_position(self.graph.graph.node_indices().filter(|i| {
            self.graph.graph[*i].name == name && !self.graph.graph[*i].node_type.is_doc()
        }))
    }

    /// The first of `candidates` by path, then line, then byte offset, then index.
    fn first_by_position(&self, candidates: impl Iterator<Item = NodeIndex>) -> Option<NodeIndex> {
        candidates.min_by(|a, b| {
            let (x, y) = (&self.graph.graph[*a], &self.graph.graph[*b]);
            (&x.path, x.line, x.start_byte, a).cmp(&(&y.path, y.line, y.start_byte, b))
        })
    }

    /// What the target of `plan` depends on (outgoing edges), each node at its
    /// shortest distance in edges, ordered by depth, path, line and name.
    pub(crate) fn plan_walk(&self, name: &str) -> Vec<(usize, NodeIndex)> {
        let Some(start) = self.plan_target(name) else {
            return Vec::new();
        };
        let mut dist: HashMap<NodeIndex, usize> = HashMap::from([(start, 0)]);
        let mut queue = VecDeque::from([start]);
        while let Some(current) = queue.pop_front() {
            let next = dist[&current] + 1;
            for neighbor in self
                .graph
                .graph
                .neighbors_directed(current, petgraph::Direction::Outgoing)
            {
                if let std::collections::hash_map::Entry::Vacant(slot) = dist.entry(neighbor) {
                    slot.insert(next);
                    queue.push_back(neighbor);
                }
            }
        }
        let mut reached: Vec<(usize, NodeIndex)> = dist
            .into_iter()
            .filter(|&(idx, _)| idx != start)
            .map(|(idx, depth)| (depth, idx))
            .collect();
        reached.sort_by(|a, b| {
            let (x, y) = (&self.graph.graph[a.1], &self.graph.graph[b.1]);
            a.0.cmp(&b.0)
                .then_with(|| x.path.cmp(&y.path))
                .then_with(|| x.line.cmp(&y.line))
                .then_with(|| x.name.cmp(&y.name))
                .then_with(|| x.kind.cmp(&y.kind))
                .then_with(|| a.1.cmp(&b.1))
        });
        reached
    }

    pub fn analyze_impact(&self, name: &str) -> Vec<NodeData> {
        self.plan_walk(name)
            .into_iter()
            .map(|(_, idx)| self.graph.graph[idx].clone())
            .collect()
    }

    /// The node a blast radius starts from: the symbol in the file, else (when the
    /// symbol is not there) the file itself. `norm_path` is already normalised.
    pub(crate) fn blast_target(
        &self,
        norm_path: &str,
        symbol: &str,
    ) -> Option<(NodeIndex, BlastMatch)> {
        let in_file = |node: &NodeData| {
            node.path == norm_path || normalize_rel_path(Path::new("."), &node.path) == norm_path
        };
        let symbol_idx = self.first_by_position(self.graph.graph.node_indices().filter(|i| {
            let node = &self.graph.graph[*i];
            in_file(node)
                && node.name == symbol
                && node.node_type != NodeType::File
                && !node.node_type.is_doc()
        }));
        if let Some(idx) = symbol_idx {
            return Some((idx, BlastMatch::Symbol));
        }
        // Try matching just by file path if symbol not found
        self.graph
            .graph
            .node_indices()
            .find(|i| {
                let node = &self.graph.graph[*i];
                in_file(node) && node.node_type == NodeType::File
            })
            .map(|idx| (idx, BlastMatch::File))
    }

    /// Who depends on the target, each at its shortest depth. See `BlastWalk`.
    pub(crate) fn blast_walk(&self, norm_path: &str, symbol: &str) -> BlastWalk {
        let Some((start, matched)) = self.blast_target(norm_path, symbol) else {
            return BlastWalk {
                target: None,
                via: None,
                dependents: Vec::new(),
            };
        };

        // A 0-1 breadth-first walk over incoming edges. Following containment upward (to
        // the enclosing symbol and the file) costs 0, so the target's structural
        // ancestors stay at distance 0 and are not dependents. Every other edge costs 1.
        let mut dist: HashMap<NodeIndex, usize> = HashMap::new();
        dist.insert(start, 0);
        let mut queue = VecDeque::from([(start, 0usize)]);
        // A method is reached through its type: no `use` names a method, so importers of
        // the enclosing symbols (and of the same-named symbols of the file, such as the
        // struct an `impl` block belongs to) are its dependents. A free function has no
        // enclosing symbol and gets no extra seeds.
        let mut via = None;
        if matched == BlastMatch::Symbol {
            let target = &self.graph.graph[start];
            let encloses = |n: &NodeData| {
                n.node_type == NodeType::Symbol
                    && n.path == target.path
                    && n.end_byte > n.start_byte
                    && n.start_byte <= target.start_byte
                    && n.end_byte >= target.end_byte
                    && (n.start_byte, n.end_byte) != (target.start_byte, target.end_byte)
            };
            let enclosing: Vec<NodeIndex> = self
                .graph
                .graph
                .node_indices()
                .filter(|i| encloses(&self.graph.graph[*i]))
                .collect();
            via = enclosing.iter().copied().min_by_key(|i| {
                let n = &self.graph.graph[*i];
                (n.start_byte, std::cmp::Reverse(n.end_byte), *i)
            });
            let names: Vec<&str> = enclosing
                .iter()
                .map(|i| self.graph.graph[*i].name.as_str())
                .collect();
            for idx in self.graph.graph.node_indices() {
                let n = &self.graph.graph[idx];
                let seed = enclosing.contains(&idx)
                    || (idx != start
                        && n.node_type == NodeType::Symbol
                        && n.path == target.path
                        && names.contains(&n.name.as_str()));
                if seed {
                    dist.insert(idx, 0);
                    queue.push_back((idx, 0));
                }
            }
        }
        // A file target stands for everything the file defines: `use crate::a::Sym` links
        // to the symbol, not the file, so the walk also starts from each of its symbols.
        if matched == BlastMatch::File {
            let file_path = self.graph.graph[start].path.clone();
            for idx in self.graph.graph.node_indices() {
                let node = &self.graph.graph[idx];
                if idx != start && node.path == file_path && !node.node_type.is_doc() {
                    dist.insert(idx, 0);
                    queue.push_back((idx, 0));
                }
            }
        }
        while let Some((current, d)) = queue.pop_front() {
            if dist.get(&current).is_some_and(|&best| best < d) {
                continue;
            }
            for edge in self
                .graph
                .graph
                .edges_directed(current, petgraph::Direction::Incoming)
            {
                let neighbor = edge.source();
                // Documentation that mentions a symbol is not a code dependent; `doc_links` lists it.
                if self.is_doc_node(neighbor) {
                    continue;
                }
                // A file holds its symbols through `Contains`, and its topics, tables and
                // environment variables through the role edges (`Produces`, `Consumes`,
                // `Configures`, `References`). All of these are structure, not dependence.
                let free = *edge.weight() == EdgeType::Contains
                    || (*edge.weight() != EdgeType::Imports && {
                        let (from, to) = (&self.graph.graph[neighbor], &self.graph.graph[current]);
                        from.node_type == NodeType::File && from.path == to.path
                    });
                let next = if free { d } else { d + 1 };
                if dist.get(&neighbor).is_none_or(|&best| next < best) {
                    dist.insert(neighbor, next);
                    if free {
                        queue.push_front((neighbor, next));
                    } else {
                        queue.push_back((neighbor, next));
                    }
                }
            }
        }

        let mut dependents: Vec<(usize, NodeIndex)> = dist
            .into_iter()
            .filter(|&(_, depth)| depth > 0)
            .map(|(idx, depth)| (depth, idx))
            .collect();
        dependents.sort_by(|a, b| {
            let (x, y) = (&self.graph.graph[a.1], &self.graph.graph[b.1]);
            a.0.cmp(&b.0)
                .then_with(|| x.path.cmp(&y.path))
                .then_with(|| x.line.cmp(&y.line))
                .then_with(|| x.name.cmp(&y.name))
                .then_with(|| x.kind.cmp(&y.kind))
                .then_with(|| a.1.cmp(&b.1))
        });
        BlastWalk {
            target: Some((start, matched)),
            via,
            dependents,
        }
    }

    /// The dependents of a symbol (or, when it is not in the file, of the file), by
    /// depth, then path, line and name.
    pub fn check_blast_radius(&self, path: &str, symbol: &str) -> Vec<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        self.blast_walk(&norm_path, symbol)
            .dependents
            .into_iter()
            .map(|(_, idx)| self.graph.graph[idx].clone())
            .collect()
    }

    /// The symbol of this name in the file; of several, the first by line.
    pub fn find_symbol_in_path(&self, path: &str, name: &str) -> Option<NodeData> {
        let norm_path = normalize_rel_path(Path::new("."), path);
        self.first_by_position(self.graph.graph.node_indices().filter(|i| {
            let n = &self.graph.graph[*i];
            n.node_type != NodeType::File
                && !n.node_type.is_doc()
                && (n.path == norm_path || normalize_rel_path(Path::new("."), &n.path) == norm_path)
                && n.name == name
        }))
        .map(|idx| self.graph.graph[idx].clone())
    }

    /// Files whose path contains `query`, by path.
    pub fn find_files(&self, query: &str) -> Vec<NodeData> {
        let query_lower = query.to_lowercase();
        let mut files: Vec<NodeData> = self
            .graph
            .graph
            .node_weights()
            .filter(|n| {
                n.node_type == NodeType::File && n.path.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files
    }

    pub fn get_all_file_paths(&self) -> Vec<String> {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.node_type == NodeType::File)
            .map(|n| n.path.clone())
            .collect()
    }

    /// A documentation node, or a Markdown file (whose own edges are doc -> code links).
    pub(crate) fn is_doc_node(&self, idx: petgraph::graph::NodeIndex) -> bool {
        let node = &self.graph.graph[idx];
        node.node_type.is_doc() || (node.node_type == NodeType::File && node.path.ends_with(".md"))
    }

    pub fn get_symbol_count(&self) -> usize {
        self.graph
            .graph
            .node_weights()
            .filter(|n| n.node_type != NodeType::File && !n.node_type.is_doc())
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

/// Path, then line, then name, then kind and byte offset: the order of `search` and `query`.
fn symbol_order(a: &NodeData, b: &NodeData) -> std::cmp::Ordering {
    (&a.path, a.line, &a.name, &a.kind, a.start_byte).cmp(&(
        &b.path,
        b.line,
        &b.name,
        &b.kind,
        b.start_byte,
    ))
}

/// The order within an outline: an own symbol by line, then name; an import by the
/// imported node's path, line and name.
fn symbol_order_path_first(a: &NodeData, b: &NodeData, import: bool) -> std::cmp::Ordering {
    if import {
        symbol_order(a, b)
    } else {
        (a.line, &a.name, &a.kind, a.start_byte).cmp(&(b.line, &b.name, &b.kind, b.start_byte))
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
