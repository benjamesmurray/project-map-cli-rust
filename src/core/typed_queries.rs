//! The typed result of every tool (`core::results`), built from the same
//! node-level queries the TOON formatter renders, so both formats see the same data
//! in the same order. TOON keeps the bare symbol names the index stores; the typed
//! results qualify them (`Outer.inner`).

use crate::core::doc_queries::DocLinksView;
use crate::core::git::{read_git_state, GitBudget};
use crate::core::graph::{ImportEntry, NodeData, NodeType};
use crate::core::parser::{is_code_extension, language_of_path};
use crate::core::query_engine::{BlastWalk, QueryEngine, SnippetExtractor};
use crate::core::results::*;
use crate::core::utils::{normalize_rel_path, PathTree};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Reads each file once to turn byte ranges into last lines.
struct EndLines<'a> {
    engine: &'a QueryEngine,
    /// Lookup-only cache of file text.
    sources: HashMap<String, Option<String>>,
}

impl<'a> EndLines<'a> {
    fn new(engine: &'a QueryEngine) -> Self {
        Self {
            engine,
            sources: HashMap::new(),
        }
    }

    /// `None` for a file node.
    fn of(&mut self, node: &NodeData) -> Option<usize> {
        if node.line == 0 {
            return None;
        }
        Some(self.last_line(node))
    }

    fn last_line(&mut self, node: &NodeData) -> usize {
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

/// The index's Git fields beside the Git state read at query time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitView {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub branch: Option<String>,
    pub head_now: Option<String>,
    pub dirty_now: Option<bool>,
    pub at_head: Option<bool>,
    pub error: Option<String>,
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
                end_line: ends.of(n),
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
                    end_line: if import { None } else { ends.of(&n) },
                    symbol: n.name,
                    kind: n.kind,
                    line: line_of(n.line),
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

    /// `blast_radius`: the dependents of a symbol (or of its file), each at its
    /// shortest depth, with counts over all of them. `limit` caps only the listed items.
    pub fn blast_result(
        &self,
        path: &str,
        symbol: &str,
        limit: Option<usize>,
    ) -> BlastRadiusResult {
        let norm = normalize_rel_path(Path::new("."), path);
        let walk = self.blast_walk(&norm, symbol);
        self.blast_from_walk(norm, &walk, limit)
    }

    /// The unlimited result and the dependent nodes (as stored, with bare names) of
    /// one walk, for the TOON formatter.
    pub fn blast_with_nodes(&self, path: &str, symbol: &str) -> (BlastRadiusResult, Vec<NodeData>) {
        let norm = normalize_rel_path(Path::new("."), path);
        let walk = self.blast_walk(&norm, symbol);
        let nodes = walk
            .dependents
            .iter()
            .map(|(_, idx)| self.graph.graph[*idx].clone())
            .collect();
        (self.blast_from_walk(norm, &walk, None), nodes)
    }

    fn blast_from_walk(
        &self,
        norm: String,
        walk: &BlastWalk,
        limit: Option<usize>,
    ) -> BlastRadiusResult {
        let namer = self.namer();
        let mut ends = EndLines::new(self);

        let target_file = walk
            .target
            .map(|(idx, _)| self.graph.graph[idx].path.clone());
        let direct = walk.dependents.iter().filter(|(d, _)| *d == 1).count();
        let total = walk.dependents.len();
        let mut files: Vec<&str> = walk
            .dependents
            .iter()
            .map(|(_, idx)| self.graph.graph[*idx].path.as_str())
            .filter(|p| Some(*p) != target_file.as_deref())
            .collect();
        files.sort_unstable();
        files.dedup();
        let mut languages: Vec<String> = files
            .iter()
            .filter_map(|p| language_of_path(p))
            .map(str::to_string)
            .collect();
        languages.sort();
        languages.dedup();

        let (_, truncated, listed) = limited(walk.dependents.clone(), limit);
        let items = listed
            .iter()
            .map(|&(depth, idx)| {
                let n = &self.graph.graph[idx];
                BlastItem {
                    symbol: namer.qualify(n),
                    kind: n.kind.clone(),
                    node_type: n.node_type.clone(),
                    path: n.path.clone(),
                    line: line_of(n.line),
                    end_line: ends.of(n),
                    depth,
                }
            })
            .collect();
        let (matched, target_symbol, target_line) = match walk.target {
            Some((idx, BlastMatch::Symbol)) => {
                let node = &self.graph.graph[idx];
                (
                    BlastMatch::Symbol,
                    Some(namer.qualify(node)),
                    Some(node.line),
                )
            }
            Some((idx, found)) => (found, None, line_of(self.graph.graph[idx].line)),
            None => (BlastMatch::NotFound, None, None),
        };
        BlastRadiusResult {
            target_path: norm,
            target_symbol,
            target_line,
            matched,
            via: walk.via.map(|idx| namer.qualify(&self.graph.graph[idx])),
            direct,
            transitive: total - direct,
            total,
            files_affected: files.len(),
            languages,
            truncated,
            items,
        }
    }

    /// `plan`: what the target depends on, each node at its distance in edges.
    pub fn plan_result(&self, symbol: &str, limit: Option<usize>) -> PlanResult {
        let namer = self.namer();
        let (target_symbol, target_path, target_line) = match self.plan_target(symbol) {
            Some(idx) => {
                let node = &self.graph.graph[idx];
                (
                    Some(namer.qualify(node)),
                    Some(node.path.clone()),
                    line_of(node.line),
                )
            }
            None => (None, None, None),
        };
        let (total, truncated, reached) = limited(self.plan_walk(symbol), limit);
        let items = reached
            .iter()
            .map(|&(depth, idx)| {
                let n = &self.graph.graph[idx];
                PlanItem {
                    symbol: namer.qualify(n),
                    kind: n.kind.clone(),
                    node_type: n.node_type.clone(),
                    path: n.path.clone(),
                    line: line_of(n.line),
                    depth,
                }
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

    /// The Git state of the root now, read once within the query budget, set against
    /// what the index recorded.
    pub fn git_view(&self) -> GitView {
        let meta = self.meta().cloned().unwrap_or_default();
        let now = read_git_state(Path::new(&meta.root), GitBudget::QUERY);
        let at_head = match (&meta.git_commit, &now.commit) {
            (Some(built), Some(head)) if built != head => Some(false),
            (Some(_), Some(_)) => match (meta.git_dirty, now.dirty) {
                (Some(false), Some(false)) => Some(true),
                (Some(true), _) | (_, Some(true)) => Some(false),
                _ => None,
            },
            _ => None,
        };
        GitView {
            commit: meta.git_commit,
            dirty: meta.git_dirty,
            branch: meta.git_branch,
            head_now: now.commit,
            dirty_now: now.dirty,
            at_head,
            error: now.error,
        }
    }

    /// `status` as data: the module tree to `max_depth` levels, and the Git state of
    /// the index against the tree now.
    pub fn status_result(&self, max_depth: usize, active_features: Vec<String>) -> StatusResult {
        let meta = self.meta().cloned().unwrap_or_default();
        let (tree, truncated) =
            PathTree::from_paths(&self.get_all_file_paths()).to_result(".", max_depth);
        let git = self.git_view();
        StatusResult {
            tool_version: meta.tool_version,
            schema_version: meta.schema_version,
            root: meta.root,
            built_at: meta.built_at,
            commit: git.commit,
            dirty: git.dirty,
            branch: git.branch,
            head_now: git.head_now,
            dirty_now: git.dirty_now,
            at_head: git.at_head,
            git_error: git.error,
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
            truncated,
            missing: false,
            next_step: None,
        }
    }

    /// `modules`: recursive counts per directory, for `path_prefix` and the
    /// directories up to `max_depth` levels below it, sorted by path. Root files
    /// count under `.`. A prefix that is not an indexed directory is `not_found`.
    pub fn modules_result(
        &self,
        path_prefix: Option<&str>,
        max_depth: usize,
    ) -> Result<ModulesResult, QueryError> {
        let prefix = path_prefix
            .map(|p| normalize_rel_path(Path::new("."), p))
            .map(|p| p.trim_end_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ".".to_string());

        // Counts per file path, then folded into every directory above the file.
        let mut per_file: BTreeMap<String, ModuleItem> = BTreeMap::new();
        for node in self.graph.graph.node_weights() {
            let path = node.path.trim_start_matches("./");
            let counts = per_file
                .entry(path.to_string())
                .or_insert_with(|| ModuleItem {
                    path: path.to_string(),
                    depth: 0,
                    files: 0,
                    code_files: 0,
                    symbols: 0,
                    doc_files: 0,
                    doc_sections: 0,
                });
            match node.node_type {
                NodeType::File => {
                    counts.files = 1;
                    match Path::new(path).extension().and_then(|e| e.to_str()) {
                        Some("md") => counts.doc_files = 1,
                        Some(ext) if is_code_extension(ext) => counts.code_files = 1,
                        _ => {}
                    }
                }
                NodeType::DocSection => {
                    counts.doc_sections += 1;
                    counts.doc_files = 1;
                }
                // OpenAPI files are API surface, not documents: `doc_files` counts Markdown.
                NodeType::ApiOperation | NodeType::ApiSchema => {}
                _ => counts.symbols += 1,
            }
        }

        let base = if prefix == "." {
            0
        } else {
            prefix.split('/').count()
        };
        let mut dirs: BTreeMap<String, ModuleItem> = BTreeMap::new();
        for (path, c) in &per_file {
            let parts: Vec<&str> = path.split('/').collect();
            let mut dir = String::new();
            for level in 0..parts.len() {
                // Level 0 is the root `.`; level n is the first n components.
                if level > 0 {
                    if !dir.is_empty() {
                        dir.push('/');
                    }
                    dir.push_str(parts[level - 1]);
                }
                let name = if level == 0 { "." } else { dir.as_str() };
                let in_prefix =
                    prefix == "." || name == prefix || name.starts_with(&format!("{prefix}/"));
                if !in_prefix {
                    continue;
                }
                let item = dirs.entry(name.to_string()).or_insert_with(|| ModuleItem {
                    path: name.to_string(),
                    depth: level - base,
                    files: 0,
                    code_files: 0,
                    symbols: 0,
                    doc_files: 0,
                    doc_sections: 0,
                });
                item.files += c.files;
                item.code_files += c.code_files;
                item.symbols += c.symbols;
                item.doc_files += c.doc_files;
                item.doc_sections += c.doc_sections;
            }
        }
        if prefix != "." && !dirs.contains_key(&prefix) {
            return Err(QueryError::not_found(format!(
                "Directory not indexed: {prefix}"
            )));
        }
        let items: Vec<ModuleItem> = dirs
            .into_values()
            .filter(|m| m.depth <= max_depth)
            .collect();
        Ok(ModulesResult {
            path_prefix: prefix,
            max_depth,
            total: items.len(),
            items,
        })
    }

    /// `imports`: the import statements of an external package, with `path_prefix`
    /// narrowing the files. `package` matches a package name exactly, or by prefix
    /// when it ends in `/*` or `.*` (`@sentry/*`). Ordered by path, line, module.
    /// `files` and `imports` count every match; `limit` (default 50) caps the list.
    pub fn imports_result(
        &self,
        package: &str,
        path_prefix: Option<&str>,
        limit: Option<usize>,
    ) -> Result<ImportsResult, QueryError> {
        let query = package.trim();
        if query.is_empty() || query == "/*" || query == ".*" {
            return Err(QueryError::invalid_argument(
                "Error: Provide a package name, e.g. stripe or @sentry/*",
            ));
        }
        if self.is_stale() {
            return Err(QueryError::index_stale());
        }
        let prefix_match = query.ends_with("/*") || query.ends_with(".*");
        let wanted = if prefix_match {
            &query[..query.len() - 1]
        } else {
            query
        };
        let dir = path_prefix
            .map(|p| normalize_rel_path(Path::new("."), p))
            .map(|p| p.trim_end_matches('/').to_string())
            .filter(|p| !p.is_empty() && p != ".");
        let in_dir = |path: &str| match &dir {
            None => true,
            Some(d) => path == d || path.starts_with(&format!("{d}/")),
        };

        let mut matches: Vec<&ImportEntry> = self
            .graph
            .imports
            .iter()
            .filter(|e| {
                e.package.as_deref().is_some_and(|p| {
                    if prefix_match {
                        p.starts_with(wanted)
                    } else {
                        p == wanted
                    }
                }) && in_dir(&e.path)
            })
            .collect();
        matches.sort_by(|a, b| (&a.path, a.line, &a.module).cmp(&(&b.path, b.line, &b.module)));
        let mut files: Vec<&str> = matches.iter().map(|e| e.path.as_str()).collect();
        files.dedup();
        let (total, truncated, listed) = limited(matches, Some(limit.unwrap_or(50)));
        Ok(ImportsResult {
            query: query.to_string(),
            files: files.len(),
            imports: total,
            truncated,
            items: listed
                .into_iter()
                .map(|e| ImportItem {
                    path: e.path.clone(),
                    line: e.line,
                    text: e.text.clone(),
                    module: e.module.clone(),
                    package: e.package.clone(),
                    language: e.language.clone(),
                    resolved: e.resolved,
                })
                .collect(),
        })
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

    /// The heading tree, cut to `max_depth` when given (h1 = 1, by nesting).
    pub fn doc_outline_result(
        &self,
        path: &str,
        max_depth: Option<usize>,
    ) -> Result<DocOutlineResult, QueryError> {
        let view = self
            .doc_outline(path)
            .map_err(QueryError::from_doc_message)?;
        let (view, truncated) = match max_depth {
            Some(depth) => view.cut_to_depth(depth),
            None => (view, false),
        };
        Ok(DocOutlineResult {
            path: view.path,
            frontmatter: view.meta,
            truncated,
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
