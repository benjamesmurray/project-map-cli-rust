//! Documentation queries over a loaded index: section search, outlines, section
//! text, doc <-> code links, terms and API surface. Section text is never in the
//! index; it is read from disk by byte range, like `fetch_symbol`.
//!
//! Every query returns `Err(message)` for a user-facing problem (stale index,
//! unknown path), which the tools print as is.

use crate::core::graph::{DocMeta, DocTerm, EdgeType, NodeType, STALE_INDEX_MESSAGE};
use crate::core::query_engine::QueryEngine;
use crate::core::results::end_line_of;
use crate::core::toon::ToonFormatter;
use crate::core::utils::normalize_rel_path;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Longest snippet line, in characters.
const SNIPPET_LINE_CHARS: usize = 140;

/// One ranked section of a `search_docs` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocHit {
    pub path: String,
    pub anchor: String,
    pub heading: String,
    /// Heading level, 1 for `#`.
    pub level: usize,
    /// 1-based line of the heading.
    pub line: usize,
    pub score: usize,
    /// Up to two lines of the section's own text.
    pub snippet: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocSearch {
    /// Sections matching before the limit was applied.
    pub total: usize,
    pub hits: Vec<DocHit>,
}

/// One heading of a `doc_outline`, indented by its depth in the section tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// 0 for a top section.
    pub depth: usize,
    pub heading: String,
    pub anchor: String,
    /// Heading level, 1 for `#`.
    pub level: usize,
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocOutlineView {
    pub path: String,
    pub meta: Option<DocMeta>,
    pub entries: Vec<OutlineEntry>,
}

/// The raw text of one section (subsections included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSectionView {
    pub path: String,
    pub anchor: String,
    pub heading: String,
    pub line: usize,
    pub end_line: usize,
    pub text: String,
}

/// A section (or, with an empty anchor, a whole file's pre-heading text) that
/// references a file or symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRef {
    pub doc_path: String,
    pub anchor: String,
    pub heading: String,
    /// `path` for a file, `path::name` for a symbol.
    pub target: String,
}

/// The references of one section of a Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionRefs {
    pub anchor: String,
    pub heading: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocLinksView {
    /// Code -> docs: sections that reference a file, or a symbol in it.
    CodeToDocs {
        path: String,
        symbol: Option<String>,
        refs: Vec<CodeRef>,
    },
    /// Docs -> code: what each section of a Markdown file references.
    DocsToCode {
        path: String,
        sections: Vec<SectionRefs>,
    },
}

/// The operations and schemas of one OpenAPI file, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiFileView {
    pub path: String,
    pub operations: Vec<ApiOperationView>,
    pub schemas: Vec<String>,
    /// 1-based line of each schema, parallel to `schemas`.
    pub schema_lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiOperationView {
    /// `METHOD /path`.
    pub name: String,
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub line: usize,
}

/// Terms merged over a directory subtree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocTermsView {
    pub prefix: String,
    /// Terms before the limit was applied.
    pub total: usize,
    pub terms: Vec<DocTerm>,
}

/// Anchors listed when a requested anchor is unknown.
const ANCHORS_LISTED: usize = 30;

/// The query, run and rendered as the text the MCP tools and CLI subcommands
/// print: TOON on success, the user-facing message on `Err`.
impl QueryEngine {
    pub fn render_doc_search(&self, query: &str, limit: usize, max_chars: usize) -> String {
        self.search_docs(query, limit)
            .map(|r| ToonFormatter::format_doc_search(query, &r, max_chars))
            .unwrap_or_else(|e| e)
    }

    pub fn render_doc_outline(&self, path: &str, max_chars: usize) -> String {
        self.doc_outline(path)
            .map(|v| ToonFormatter::format_doc_outline(&v, max_chars))
            .unwrap_or_else(|e| e)
    }

    pub fn render_doc_section(&self, path: &str, anchor: &str, max_chars: usize) -> String {
        self.doc_section(path, anchor)
            .map(|v| ToonFormatter::format_doc_section(&v, max_chars))
            .unwrap_or_else(|e| e)
    }

    pub fn render_doc_links(&self, path: &str, symbol: Option<&str>, max_chars: usize) -> String {
        self.doc_links(path, symbol)
            .map(|v| ToonFormatter::format_doc_links(&v, max_chars))
            .unwrap_or_else(|e| e)
    }

    pub fn render_doc_terms(
        &self,
        path_prefix: Option<&str>,
        limit: usize,
        max_chars: usize,
    ) -> String {
        self.doc_terms(path_prefix, limit)
            .map(|v| ToonFormatter::format_doc_terms(&v, max_chars))
            .unwrap_or_else(|e| e)
    }

    pub fn render_api_surface(&self, path: Option<&str>, max_chars: usize) -> String {
        self.api_surface(path)
            .map(|v| ToonFormatter::format_api_surface(&v, max_chars))
            .unwrap_or_else(|e| e)
    }
}

impl QueryEngine {
    /// The shared stale message for an index without schema 3 doc data.
    fn require_current(&self) -> Result<(), String> {
        if self.lacks_docs() {
            Err(STALE_INDEX_MESSAGE.to_string())
        } else {
            Ok(())
        }
    }

    /// `./x`, `x` and absolute forms (under the index root) give the indexed path.
    pub(crate) fn norm_path(&self, path: &str) -> String {
        let root = self
            .graph
            .meta
            .as_ref()
            .map_or(Path::new("."), |m| Path::new(&m.root));
        normalize_rel_path(root, path)
    }

    /// File text for an indexed path: as given (relative to the working directory,
    /// like `fetch_symbol`), else under the index root.
    pub(crate) fn read_source(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok().or_else(|| {
            let root = &self.graph.meta.as_ref()?.root;
            std::fs::read_to_string(Path::new(root).join(path)).ok()
        })
    }

    /// Where to open an indexed path: as given if it exists (relative to the working
    /// directory, like `fetch_symbol`), else under the index root.
    pub(crate) fn openable_path(&self, path: &str) -> String {
        if Path::new(path).exists() {
            return path.to_string();
        }
        match self.graph.meta.as_ref() {
            Some(m) if Path::new(&m.root).join(path).exists() => {
                Path::new(&m.root).join(path).to_string_lossy().into_owned()
            }
            _ => path.to_string(),
        }
    }

    /// The indexed file node for `path` (any accepted spelling).
    pub(crate) fn file_node(&self, path: &str) -> Option<NodeIndex> {
        let norm = self.norm_path(path);
        self.graph.graph.node_indices().find(|&i| {
            let n = &self.graph.graph[i];
            n.node_type == NodeType::File && n.path == norm
        })
    }

    /// A Markdown file's node, or the message for an unknown or non-Markdown path.
    pub(crate) fn markdown_node(&self, path: &str) -> Result<NodeIndex, String> {
        self.file_node(path)
            .filter(|&i| self.graph.graph[i].path.ends_with(".md"))
            .ok_or_else(|| {
                format!(
                    "Unknown document: {} is not an indexed Markdown file. \
                     Use `query` with a directory path to list indexed files.",
                    self.norm_path(path)
                )
            })
    }

    /// DocSection nodes grouped by file, in document order.
    pub(crate) fn sections_by_file(&self) -> BTreeMap<&str, Vec<NodeIndex>> {
        let mut by_file: BTreeMap<&str, Vec<NodeIndex>> = BTreeMap::new();
        for idx in self.graph.graph.node_indices() {
            let node = &self.graph.graph[idx];
            if node.node_type == NodeType::DocSection {
                by_file.entry(node.path.as_str()).or_default().push(idx);
            }
        }
        for sections in by_file.values_mut() {
            sections.sort_by_key(|&i| self.graph.graph[i].start_byte);
        }
        by_file
    }

    /// Ranks sections for `query`: every keyword must occur in the heading, the
    /// section's own text (up to the next heading) or, for a file's first section,
    /// its frontmatter id, title and tags. Heading hits weigh most, then
    /// frontmatter, then text; a whole-phrase hit adds a bonus. Ties break by path
    /// then position. Text is read from disk; a file that changed since indexing
    /// still matches on headings and frontmatter, with no snippet.
    pub fn search_docs(&self, query: &str, limit: usize) -> Result<DocSearch, String> {
        self.require_current()?;
        let keywords: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if keywords.is_empty() {
            return Ok(DocSearch::default());
        }
        let phrase = keywords.join(" ");

        let mut ranked: Vec<(usize, &str, usize, DocHit)> = Vec::new();
        for (path, sections) in self.sections_by_file() {
            let text = self.read_source(path);
            let front = self.graph.doc_meta.get(path).map(|m| {
                let mut parts: Vec<&str> =
                    m.id.iter()
                        .chain(m.title.iter())
                        .map(String::as_str)
                        .collect();
                parts.extend(m.tags.iter().map(String::as_str));
                parts.join(" ").to_lowercase()
            });
            for (n, &idx) in sections.iter().enumerate() {
                let node = &self.graph.graph[idx];
                // Own text: up to the next heading of any level.
                let own_end = sections
                    .get(n + 1)
                    .map_or(node.end_byte, |&next| self.graph.graph[next].start_byte)
                    .min(node.end_byte);
                let body = text
                    .as_deref()
                    .and_then(|t| t.get(node.start_byte..own_end))
                    .map(body_after_heading)
                    .unwrap_or("");
                let heading = node.name.to_lowercase();
                let body_lower = body.to_lowercase();
                let front = if n == 0 { front.as_deref() } else { None };

                let mut score = 0;
                let mut all = true;
                for kw in &keywords {
                    let in_heading = heading.contains(kw.as_str());
                    let in_front = front.is_some_and(|f| f.contains(kw.as_str()));
                    let in_body = body_lower.contains(kw.as_str());
                    if !(in_heading || in_front || in_body) {
                        all = false;
                        break;
                    }
                    score += 6 * usize::from(in_heading)
                        + 4 * usize::from(in_front)
                        + 2 * usize::from(in_body);
                }
                if !all {
                    continue;
                }
                if keywords.len() > 1 {
                    score += 8 * usize::from(heading.contains(&phrase));
                    score += 3 * usize::from(body_lower.contains(&phrase));
                }
                ranked.push((
                    score,
                    path,
                    node.start_byte,
                    DocHit {
                        path: path.to_string(),
                        anchor: node.role.clone().unwrap_or_default(),
                        heading: node.name.clone(),
                        level: heading_level(&node.kind),
                        line: node.line,
                        score,
                        snippet: snippet(body, &keywords),
                    },
                ));
            }
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)).then(a.2.cmp(&b.2)));
        let total = ranked.len();
        let hits = ranked.into_iter().take(limit).map(|r| r.3).collect();
        Ok(DocSearch { total, hits })
    }

    /// Heading tree of a Markdown file, with its frontmatter if it has any.
    /// `path` may be `./x`, `x` or absolute under the index root.
    pub fn doc_outline(&self, path: &str) -> Result<DocOutlineView, String> {
        self.require_current()?;
        let file = self.markdown_node(path)?;
        let mut entries = Vec::new();
        let norm = self.graph.graph[file].path.clone();
        let source = self.read_source(&norm);
        self.walk_sections(file, 0, source.as_deref(), &mut entries);
        Ok(DocOutlineView {
            meta: self.graph.doc_meta.get(&norm).cloned(),
            path: norm,
            entries,
        })
    }

    /// The raw text of a section, subsections included, read from disk by its
    /// indexed byte range. `anchor` may carry a leading `#`. If the file no longer
    /// matches the index (range out of bounds, inside a character, or not at a
    /// heading) the answer is a stale-range message, not wrong text.
    pub fn doc_section(&self, path: &str, anchor: &str) -> Result<DocSectionView, String> {
        self.require_current()?;
        let file = self.markdown_node(path)?;
        let norm = self.graph.graph[file].path.clone();
        let anchor = anchor.trim().trim_start_matches('#');
        let sections = self.sections_by_file();
        let in_file = sections.get(norm.as_str()).map_or(&[][..], Vec::as_slice);
        let Some(&idx) = in_file
            .iter()
            .find(|&&i| self.graph.graph[i].role.as_deref() == Some(anchor))
        else {
            let anchors: Vec<_> = in_file
                .iter()
                .filter_map(|&i| self.graph.graph[i].role.as_deref())
                .collect();
            let more = anchors.len().saturating_sub(ANCHORS_LISTED);
            return Err(format!(
                "Unknown anchor #{anchor} in {norm}. Anchors: {}{}",
                if anchors.is_empty() {
                    "(none)".to_string()
                } else {
                    anchors[..anchors.len() - more].join(", ")
                },
                if more > 0 {
                    format!(" ... and {more} more")
                } else {
                    String::new()
                }
            ));
        };
        let node = &self.graph.graph[idx];
        let stale = || {
            format!(
                "Stale range: {norm} changed since it was indexed, so section #{anchor} cannot be \
                 read. Run `refresh` (the MCP `init` tool) to re-index."
            )
        };
        let text = self.read_source(&norm).ok_or_else(stale)?;
        let section = text
            .get(node.start_byte..node.end_byte)
            .filter(|s| starts_at_heading(s))
            .ok_or_else(stale)?;
        Ok(DocSectionView {
            line: node.line,
            end_line: end_line_of(&text, node.line, node.start_byte, node.end_byte),
            path: norm,
            anchor: anchor.to_string(),
            heading: node.name.clone(),
            text: section.trim_end().to_string(),
        })
    }

    /// Doc <-> code links. For a Markdown file: each section with the files and
    /// symbols it references. For any other indexed file: the sections that
    /// reference it, or only `symbol` in it if given; without a symbol, references
    /// to the file's symbols are included and named.
    pub fn doc_links(&self, path: &str, symbol: Option<&str>) -> Result<DocLinksView, String> {
        self.require_current()?;
        let file = self
            .file_node(path)
            .ok_or_else(|| format!("Unknown file: {} is not indexed.", self.norm_path(path)))?;
        let norm = self.graph.graph[file].path.clone();
        if norm.ends_with(".md") {
            return Ok(self.docs_to_code(file, norm));
        }

        let targets: Vec<NodeIndex> = match symbol.filter(|s| !s.is_empty()) {
            Some(name) => {
                let found: Vec<_> = self
                    .contained(file)
                    .into_iter()
                    .filter(|&i| self.graph.graph[i].name == name)
                    .collect();
                if found.is_empty() {
                    return Err(format!("Unknown symbol: {name} in {norm}."));
                }
                found
            }
            None => std::iter::once(file).chain(self.contained(file)).collect(),
        };
        let mut refs = Vec::new();
        for target in targets {
            let label = self.target_label(target);
            for edge in self
                .graph
                .graph
                .edges_directed(target, petgraph::Direction::Incoming)
            {
                let from = edge.source();
                if *edge.weight() != EdgeType::References || !self.is_doc_node(from) {
                    continue;
                }
                let source = &self.graph.graph[from];
                let in_section = source.node_type == NodeType::DocSection;
                refs.push((
                    source.path.clone(),
                    if in_section { source.start_byte } else { 0 },
                    CodeRef {
                        doc_path: source.path.clone(),
                        anchor: if in_section {
                            source.role.clone().unwrap_or_default()
                        } else {
                            String::new()
                        },
                        heading: if in_section {
                            source.name.clone()
                        } else {
                            "(before first heading)".to_string()
                        },
                        target: label.clone(),
                    },
                ));
            }
        }
        refs.sort_by(|a, b| (&a.0, a.1, &a.2.target).cmp(&(&b.0, b.1, &b.2.target)));
        refs.dedup_by(|a, b| a.2 == b.2);
        Ok(DocLinksView::CodeToDocs {
            path: norm,
            symbol: symbol.filter(|s| !s.is_empty()).map(str::to_string),
            refs: refs.into_iter().map(|r| r.2).collect(),
        })
    }

    /// Nodes a file contains (symbols and, for specs, API nodes).
    fn contained(&self, file: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .graph
            .edges(file)
            .filter(|e| *e.weight() == EdgeType::Contains)
            .map(|e| e.target())
            .collect()
    }

    fn target_label(&self, idx: NodeIndex) -> String {
        let n = &self.graph.graph[idx];
        if n.node_type == NodeType::File {
            n.path.clone()
        } else {
            format!("{}::{}", n.path, n.name)
        }
    }

    fn docs_to_code(&self, file: NodeIndex, norm: String) -> DocLinksView {
        let mut sections = Vec::new();
        let mut owners: Vec<NodeIndex> = vec![file];
        let mut in_file: Vec<NodeIndex> = self
            .sections_by_file()
            .get(norm.as_str())
            .cloned()
            .unwrap_or_default();
        in_file.sort_by_key(|&i| self.graph.graph[i].start_byte);
        owners.extend(in_file);
        for owner in owners {
            let mut targets: Vec<String> = self
                .graph
                .graph
                .edges(owner)
                .filter(|e| *e.weight() == EdgeType::References)
                .map(|e| self.target_label(e.target()))
                .collect();
            targets.sort();
            targets.dedup();
            if targets.is_empty() {
                continue;
            }
            let node = &self.graph.graph[owner];
            let is_file = node.node_type == NodeType::File;
            sections.push(SectionRefs {
                anchor: if is_file {
                    String::new()
                } else {
                    node.role.clone().unwrap_or_default()
                },
                heading: if is_file {
                    "(before first heading)".to_string()
                } else {
                    node.name.clone()
                },
                targets,
            });
        }
        DocLinksView::DocsToCode {
            path: norm,
            sections,
        }
    }

    /// Recurring terms under `path_prefix` (every directory if `None` or empty):
    /// the stored per-directory lists merged, counts re-summed, ordered by count
    /// descending then term ascending, at most `limit` terms and 5 sections each.
    pub fn doc_terms(
        &self,
        path_prefix: Option<&str>,
        limit: usize,
    ) -> Result<DocTermsView, String> {
        self.require_current()?;
        let prefix = match path_prefix.map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => self.norm_path(p).trim_end_matches('/').to_string(),
            None => String::new(),
        };
        let everything = prefix.is_empty() || prefix == ".";
        /// (summed count, (path, anchor) sections seen)
        type Merged<'a> = (u32, Vec<&'a (String, String)>);
        let mut merged: HashMap<&str, Merged> = HashMap::new();
        for (dir, terms) in &self.graph.doc_terms {
            let inside = everything
                || dir == &prefix
                || dir
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| rest.starts_with('/'));
            if !inside {
                continue;
            }
            for t in terms {
                let entry = merged.entry(t.term.as_str()).or_default();
                entry.0 += t.count;
                entry.1.extend(t.sections.iter());
            }
        }
        let mut terms: Vec<DocTerm> = merged
            .into_iter()
            .map(|(term, (count, mut sections))| {
                // Stable: sections of one file keep their stored (byte) order.
                sections.sort_by(|a, b| a.0.cmp(&b.0));
                let mut seen = HashSet::new();
                DocTerm {
                    term: term.to_string(),
                    count,
                    sections: sections
                        .into_iter()
                        .filter(|s| seen.insert(*s))
                        .take(5)
                        .cloned()
                        .collect(),
                }
            })
            .collect();
        terms.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.term.cmp(&b.term)));
        let total = terms.len();
        terms.truncate(limit);
        Ok(DocTermsView {
            prefix: if everything { ".".to_string() } else { prefix },
            total,
            terms,
        })
    }

    /// Operations (`METHOD /path (operationId)` with summary) and schema names of
    /// each OpenAPI file, or of `path` only.
    pub fn api_surface(&self, path: Option<&str>) -> Result<Vec<ApiFileView>, String> {
        self.require_current()?;
        let wanted = path
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| self.norm_path(p));
        let mut files: BTreeMap<&str, Vec<NodeIndex>> = BTreeMap::new();
        for idx in self.graph.graph.node_indices() {
            let node = &self.graph.graph[idx];
            if matches!(node.node_type, NodeType::ApiOperation | NodeType::ApiSchema)
                && wanted.as_deref().is_none_or(|w| w == node.path)
            {
                files.entry(node.path.as_str()).or_default().push(idx);
            }
        }
        if files.is_empty() {
            return Err(match wanted {
                Some(w) => format!("No API operations or schemas are indexed for {w}."),
                None => "No OpenAPI files are indexed.".to_string(),
            });
        }
        Ok(files
            .into_iter()
            .map(|(path, mut nodes)| {
                nodes.sort_by_key(|&i| self.graph.graph[i].start_byte);
                let mut view = ApiFileView {
                    path: path.to_string(),
                    operations: Vec::new(),
                    schemas: Vec::new(),
                    schema_lines: Vec::new(),
                };
                for i in nodes {
                    let n = &self.graph.graph[i];
                    if n.node_type == NodeType::ApiOperation {
                        view.operations.push(ApiOperationView {
                            name: n.name.clone(),
                            operation_id: n.role.clone(),
                            summary: n.docstring.clone(),
                            line: n.line,
                        });
                    } else {
                        view.schemas.push(n.name.clone());
                        view.schema_lines.push(n.line);
                    }
                }
                view
            })
            .collect())
    }

    /// Depth-first over `Contains` edges to DocSection children, in document order.
    fn walk_sections(
        &self,
        parent: NodeIndex,
        depth: usize,
        source: Option<&str>,
        out: &mut Vec<OutlineEntry>,
    ) {
        let mut kids: Vec<NodeIndex> = self
            .graph
            .graph
            .edges(parent)
            .filter(|e| *e.weight() == EdgeType::Contains)
            .map(|e| e.target())
            .filter(|&t| self.graph.graph[t].node_type == NodeType::DocSection)
            .collect();
        kids.sort_by_key(|&k| self.graph.graph[k].start_byte);
        for kid in kids {
            let node = &self.graph.graph[kid];
            out.push(OutlineEntry {
                depth,
                heading: node.name.clone(),
                anchor: node.role.clone().unwrap_or_default(),
                level: heading_level(&node.kind),
                line: node.line,
                end_line: source.map_or(node.line, |text| {
                    end_line_of(text, node.line, node.start_byte, node.end_byte)
                }),
            });
            self.walk_sections(kid, depth + 1, source, out);
        }
    }
}

/// Heading level from a section node's kind (`h1` to `h6`).
fn heading_level(kind: &str) -> usize {
    kind.strip_prefix('h')
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

/// True if `s` begins with an ATX heading (`#` after at most three spaces) or a
/// setext heading (its second line is `---` or `===` only).
fn starts_at_heading(s: &str) -> bool {
    let mut lines = s.lines();
    let first = lines.next().unwrap_or("");
    let indent = first.len() - first.trim_start_matches(' ').len();
    if indent <= 3 && first.trim_start().starts_with('#') {
        return true;
    }
    let second = lines.next().unwrap_or("").trim();
    !second.is_empty() && second.chars().all(|c| c == '-' || c == '=')
}

/// A section's text after its heading line (and a setext underline, if any).
fn body_after_heading(own: &str) -> &str {
    let rest = own.split_once('\n').map_or("", |(_, rest)| rest);
    let underline = rest.split('\n').next().unwrap_or("").trim();
    if !underline.is_empty() && underline.chars().all(|c| c == '-' || c == '=') {
        return rest.split_once('\n').map_or("", |(_, r)| r);
    }
    rest
}

/// Two lines: the first line mentioning a keyword and the next non-empty line,
/// or the first two non-empty lines if none does.
fn snippet(body: &str, keywords: &[String]) -> Vec<String> {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines
        .iter()
        .position(|l| {
            let lower = l.to_lowercase();
            keywords.iter().any(|k| lower.contains(k.as_str()))
        })
        .unwrap_or(0);
    lines[start.min(lines.len())..]
        .iter()
        .take(2)
        .map(|l| {
            if l.chars().count() > SNIPPET_LINE_CHARS {
                let cut: String = l.chars().take(SNIPPET_LINE_CHARS).collect();
                format!("{cut}…")
            } else {
                (*l).to_string()
            }
        })
        .collect()
}
