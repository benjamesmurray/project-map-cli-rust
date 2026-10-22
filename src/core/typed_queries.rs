//! The typed result of every tool (`core::results`), built from the same
//! node-level queries the TOON formatter renders, so both formats see the same data
//! in the same order. TOON keeps the bare symbol names the index stores; the typed
//! results qualify them (`Outer.inner`).

use crate::core::doc_queries::DocLinksView;
use crate::core::graph::NodeData;
use crate::core::query_engine::{QueryEngine, SnippetExtractor};
use crate::core::results::*;
use crate::core::utils::{normalize_rel_path, PathTree};
use std::collections::HashMap;
use std::path::Path;

/// Reads each file once to turn byte ranges into last lines.
struct EndLines<'a> {
    engine: &'a QueryEngine,
    sources: HashMap<String, Option<String>>,
}

impl<'a> EndLines<'a> {
    fn new(engine: &'a QueryEngine) -> Self {
        Self {
            engine,
            sources: HashMap::new(),
        }
    }

    fn of(&mut self, node: &NodeData) -> usize {
        let engine = self.engine;
        let source = self
            .sources
            .entry(node.path.clone())
            .or_insert_with(|| engine.read_source(&node.path));
        source.as_deref().map_or(node.line, |text| {
            end_line_of(text, node.line, node.start_byte, node.end_byte)
        })
    }
}

fn limited<T>(mut all: Vec<T>, limit: Option<usize>) -> (usize, bool, Vec<T>) {
    let total = all.len();
    match limit {
        Some(l) if l < total => {
            all.truncate(l);
            (total, true, all)
        }
        _ => (total, false, all),
    }
}

impl QueryEngine {
    /// The envelope's `index` object for a loaded engine.
    pub fn index_info(&self) -> IndexInfo {
        IndexInfo::loaded(self.meta(), self.is_stale())
    }

    fn namer(&self) -> QualifiedNamer {
        QualifiedNamer::new(self.graph.graph.node_weights())
    }

    /// `search` and the query form of `query`.
    pub fn entity_list(
        &self,
        query: &str,
        preview_lines: Option<usize>,
        limit: Option<usize>,
    ) -> EntityList {
        let (total, truncated, nodes) = limited(self.find_symbols(query), limit);
        let namer = self.namer();
        let mut ends = EndLines::new(self);
        let mut extractor = SnippetExtractor::new();
        let items = nodes
            .iter()
            .map(|n| SymbolItem {
                symbol: namer.qualify(n),
                kind: n.kind.clone(),
                node_type: n.node_type.clone(),
                path: n.path.clone(),
                line: n.line,
                end_line: Some(ends.of(n)),
                role: n.role.clone(),
                preview: preview_lines
                    .filter(|_| n.line > 0)
                    .and_then(|lines| {
                        extractor.extract(&self.openable_path(&n.path), n.line, lines)
                    })
                    .map(|p| p.formatted),
            })
            .collect();
        EntityList {
            query: query.to_string(),
            total,
            truncated,
            items,
        }
    }

    /// The path form of `query`: a file's outline. An unindexed file is `not_found`.
    pub fn outline_result(&self, path: &str) -> Result<FileOutlineResult, QueryError> {
        if self.meta().is_none() {
            return Err(QueryError::index_missing());
        }
        let norm = normalize_rel_path(Path::new("."), path);
        // "." lists every file, as `get_file_outline` does.
        let whole_tree = norm == ".";
        if !whole_tree && self.outline_file_node(&norm).is_none() {
            return Err(QueryError::not_found(format!("File not indexed: {norm}")));
        }
        let mut ends = EndLines::new(self);
        let items = self
            .get_file_outline(&norm)
            .into_iter()
            .map(|n| {
                let import = n.role.as_deref() == Some("Import");
                OutlineItem {
                    end_line: (!import).then(|| ends.of(&n)),
                    symbol: n.name,
                    kind: n.kind,
                    line: n.line,
                    role: n.role,
                    path: (import || whole_tree).then_some(n.path),
                }
            })
            .collect();
        Ok(FileOutlineResult { path: norm, items })
    }

    /// The filename form of `query`.
    pub fn files_result(&self, query: &str, limit: Option<usize>) -> FileList {
        let paths = self.find_files(query).into_iter().map(|n| n.path).collect();
        let (total, truncated, paths) = limited(paths, limit);
        FileList {
            query: query.to_string(),
            total,
            truncated,
            paths,
        }
    }

    /// `fetch_symbol`: the symbol's source, read by its byte range.
    pub fn fetch_result(&self, path: &str, symbol: &str) -> Result<FetchResult, QueryError> {
        let norm = normalize_rel_path(Path::new("."), path);
        let node = self.find_symbol_in_path(&norm, symbol).ok_or_else(|| {
            QueryError::not_found(format!("Symbol not found: {symbol} in {norm}"))
        })?;
        let text = self
            .read_source(&node.path)
            .ok_or_else(|| QueryError::io_error("Error: Could not read file"))?;
        let source = text
            .get(node.start_byte..node.end_byte)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| QueryError::io_error("Error: Byte range or line out of bounds"))?;
        Ok(FetchResult {
            path: norm,
            symbol: self.namer().qualify(&node),
            line: node.line,
            end_line: end_line_of(&text, node.line, node.start_byte, node.end_byte),
            source: source.to_string(),
        })
    }

    /// `blast_radius`. The walk is the 0.3.0 one until the breadth-first walk
    /// replaces it, so depth and the direct / transitive counts are not set yet.
    pub fn blast_result(
        &self,
        path: &str,
        symbol: &str,
        limit: Option<usize>,
    ) -> BlastRadiusResult {
        let norm = normalize_rel_path(Path::new("."), path);
        let target = self.blast_target(&norm, symbol);
        let namer = self.namer();
        let mut ends = EndLines::new(self);
        let (total, truncated, nodes) = limited(self.check_blast_radius(&norm, symbol), limit);
        let items = nodes
            .iter()
            .map(|n| BlastItem {
                symbol: namer.qualify(n),
                kind: n.kind.clone(),
                node_type: n.node_type.clone(),
                path: n.path.clone(),
                line: n.line,
                end_line: Some(ends.of(n)),
                depth: None,
            })
            .collect();
        let (matched, target_symbol, target_line) = match target {
            Some((idx, BlastMatch::Symbol)) => {
                let node = &self.graph.graph[idx];
                (
                    BlastMatch::Symbol,
                    Some(namer.qualify(node)),
                    Some(node.line),
                )
            }
            Some((idx, found)) => (found, None, Some(self.graph.graph[idx].line)),
            None => (BlastMatch::NotFound, None, None),
        };
        BlastRadiusResult {
            target_path: norm,
            target_symbol,
            target_line,
            matched,
            direct: None,
            transitive: None,
            total,
            files_affected: None,
            languages: Vec::new(),
            truncated,
            items,
        }
    }

    /// `plan`. As for `blast_result`, depth is not set yet.
    pub fn plan_result(&self, symbol: &str, limit: Option<usize>) -> PlanResult {
        let namer = self.namer();
        let (target_symbol, target_path, target_line) = match self.plan_target(symbol) {
            Some(idx) => {
                let node = &self.graph.graph[idx];
                (
                    Some(namer.qualify(node)),
                    Some(node.path.clone()),
                    Some(node.line),
                )
            }
            None => (None, None, None),
        };
        let (total, truncated, nodes) = limited(self.analyze_impact(symbol), limit);
        let items = nodes
            .iter()
            .map(|n| PlanItem {
                symbol: namer.qualify(n),
                kind: n.kind.clone(),
                node_type: n.node_type.clone(),
                path: n.path.clone(),
                line: n.line,
                depth: None,
            })
            .collect();
        PlanResult {
            requested: symbol.to_string(),
            target_symbol,
            target_path,
            target_line,
            total,
            truncated,
            items,
        }
    }

    /// `status` as data. The Git fields stay `None` until the index records them.
    pub fn status_result(&self, max_depth: usize, active_features: Vec<String>) -> StatusResult {
        let meta = self.meta().cloned().unwrap_or_default();
        let (tree, _) = PathTree::from_paths(&self.get_all_file_paths()).to_result(".", max_depth);
        StatusResult {
            tool_version: meta.tool_version,
            schema_version: meta.schema_version,
            root: meta.root,
            built_at: meta.built_at,
            commit: None,
            dirty: None,
            branch: None,
            head_now: None,
            dirty_now: None,
            at_head: None,
            git_error: None,
            stale: self.is_stale(),
            files: meta.file_count,
            symbols: meta.symbol_count,
            edges: meta.edge_count,
            doc_files: meta.doc_file_count,
            doc_sections: meta.section_count,
            doc_references: meta.reference_count,
            api_operations: meta.api_operation_count,
            tree,
            active_features,
        }
    }

    // --- Documentation tools ---

    pub fn doc_search_result(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<DocSearchResult, QueryError> {
        let found = self
            .search_docs(query, limit)
            .map_err(QueryError::from_doc_message)?;
        Ok(DocSearchResult {
            query: query.to_string(),
            total: found.total,
            truncated: found.total > found.hits.len(),
            items: found
                .hits
                .into_iter()
                .map(|h| DocSearchItem {
                    path: h.path,
                    anchor: h.anchor,
                    heading: h.heading,
                    level: h.level,
                    line: h.line,
                    score: h.score,
                    snippet: h.snippet,
                })
                .collect(),
        })
    }

    pub fn doc_outline_result(&self, path: &str) -> Result<DocOutlineResult, QueryError> {
        let view = self
            .doc_outline(path)
            .map_err(QueryError::from_doc_message)?;
        Ok(DocOutlineResult {
            path: view.path,
            frontmatter: view.meta,
            truncated: false,
            items: view
                .entries
                .into_iter()
                .map(|e| DocOutlineItem {
                    anchor: e.anchor,
                    heading: e.heading,
                    level: e.level,
                    depth: e.depth + 1,
                    line: e.line,
                    end_line: e.end_line,
                })
                .collect(),
        })
    }

    /// The full section text. `max_chars` cuts it and says so in `truncated`.
    pub fn doc_section_result(
        &self,
        path: &str,
        anchor: &str,
        max_chars: Option<usize>,
    ) -> Result<DocSectionResult, QueryError> {
        let view = self
            .doc_section(path, anchor)
            .map_err(QueryError::from_doc_message)?;
        let (text, truncated) = match max_chars {
            Some(max) if view.text.chars().count() > max => {
                (view.text.chars().take(max).collect(), true)
            }
            _ => (view.text, false),
        };
        Ok(DocSectionResult {
            path: view.path,
            anchor: view.anchor,
            heading: view.heading,
            line: view.line,
            end_line: view.end_line,
            text,
            truncated,
        })
    }

    pub fn doc_links_result(
        &self,
        path: &str,
        symbol: Option<&str>,
    ) -> Result<DocLinksResult, QueryError> {
        let view = self
            .doc_links(path, symbol)
            .map_err(QueryError::from_doc_message)?;
        let (path, symbol, items) = match view {
            DocLinksView::CodeToDocs { path, symbol, refs } => {
                let items = refs
                    .into_iter()
                    .map(|r| {
                        let (target_path, target_symbol) = split_target(&r.target);
                        DocLinkItem {
                            direction: LinkDirection::ToDocs,
                            path: r.doc_path,
                            anchor: r.anchor,
                            heading: r.heading,
                            form: form_of(&target_symbol),
                            target_path,
                            target_symbol,
                        }
                    })
                    .collect();
                (path, symbol, items)
            }
            DocLinksView::DocsToCode { path, sections } => {
                let mut items = Vec::new();
                for section in sections {
                    for target in &section.targets {
                        let (target_path, target_symbol) = split_target(target);
                        items.push(DocLinkItem {
                            direction: LinkDirection::ToCode,
                            path: path.clone(),
                            anchor: section.anchor.clone(),
                            heading: section.heading.clone(),
                            form: form_of(&target_symbol),
                            target_path,
                            target_symbol,
                        });
                    }
                }
                (path, None, items)
            }
        };
        Ok(DocLinksResult {
            path,
            symbol,
            total: items.len(),
            items,
        })
    }

    pub fn doc_terms_result(
        &self,
        path_prefix: Option<&str>,
        limit: usize,
    ) -> Result<DocTermsResult, QueryError> {
        let view = self
            .doc_terms(path_prefix, limit)
            .map_err(QueryError::from_doc_message)?;
        Ok(DocTermsResult {
            path_prefix: view.prefix,
            total: view.total,
            truncated: view.total > view.terms.len(),
            items: view
                .terms
                .into_iter()
                .map(|t| DocTermItem {
                    term: t.term,
                    count: t.count,
                    sections: t
                        .sections
                        .into_iter()
                        .map(|(path, anchor)| SectionRef { path, anchor })
                        .collect(),
                })
                .collect(),
        })
    }

    pub fn api_surface_result(&self, path: Option<&str>) -> Result<ApiSurfaceResult, QueryError> {
        let files = self
            .api_surface(path)
            .map_err(QueryError::from_doc_message)?;
        let mut items = Vec::new();
        for file in files {
            for op in file.operations {
                let (method, route) = match op.name.split_once(' ') {
                    Some((m, r)) => (Some(m.to_string()), Some(r.to_string())),
                    None => (None, Some(op.name.clone())),
                };
                items.push(ApiSurfaceItem {
                    path: file.path.clone(),
                    item: "operation".to_string(),
                    method,
                    route,
                    operation_id: op.operation_id,
                    name: None,
                    summary: op.summary,
                    line: op.line,
                });
            }
            for (name, line) in file.schemas.into_iter().zip(file.schema_lines) {
                items.push(ApiSurfaceItem {
                    path: file.path.clone(),
                    item: "schema".to_string(),
                    method: None,
                    route: None,
                    operation_id: None,
                    name: Some(name),
                    summary: None,
                    line,
                });
            }
        }
        Ok(ApiSurfaceResult {
            total: items.len(),
            items,
        })
    }
}

/// `path` or `path::name` (a reference label) as (path, symbol).
fn split_target(label: &str) -> (String, Option<String>) {
    match label.split_once("::") {
        Some((path, name)) => (path.to_string(), Some(name.to_string())),
        None => (label.to_string(), None),
    }
}

fn form_of(symbol: &Option<String>) -> String {
    if symbol.is_some() { "symbol" } else { "file" }.to_string()
}
