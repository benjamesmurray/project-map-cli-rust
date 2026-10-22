use crate::core::doc_queries::{
    ApiFileView, DocLinksView, DocOutlineView, DocSearch, DocSectionView, DocTermsView,
};
use crate::core::graph::{EntityMatch, NodeData, NodeType, STALE_INDEX_MESSAGE};
use crate::core::query_engine::QueryEngine;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Default size cap, in characters, for the documentation tools.
pub const DEFAULT_MAX_CHARS: usize = 4000;

/// Caps `text` at `max_chars` characters. Over the cap, the text is cut at the
/// last line break inside the budget (or at the budget, on a character boundary,
/// if the kept part has none) and one marker line is appended. The marker is not
/// counted in the budget.
pub fn truncate_output(text: &str, max_chars: usize) -> String {
    let Some((cut, _)) = text.char_indices().nth(max_chars) else {
        return text.to_string();
    };
    let kept = &text[..cut];
    let kept = match kept.rfind('\n') {
        Some(newline) if newline > 0 => &kept[..newline],
        _ => kept,
    };
    format!(
        "{}\n[truncated at {} characters: raise max_chars or narrow the query to see more]",
        kept.trim_end_matches('\n'),
        max_chars
    )
}

pub struct ToonFormatter;

impl ToonFormatter {
    pub fn format_symbols(query: &str, matches: &[NodeData]) -> String {
        let mut output = format!("Resource: Symbols | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));

        for m in matches.iter().take(10) {
            output.push_str(&format!("- {} ({}) [line {}]\n", m.path, m.name, m.line));
        }

        if matches.len() > 10 {
            output.push_str(&format!("... and {} more.\n", matches.len() - 10));
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!(
                "\nNext Step: `project-map impact --fqn {}`",
                first.name
            ));
        }

        output
    }

    pub fn format_entity_matches(query: &str, matches: &[EntityMatch]) -> String {
        let mut output = format!("Resource: Symbols | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));

        let is_kafka_query = matches.iter().any(|m| m.node_type == NodeType::KafkaTopic);

        if is_kafka_query {
            let topic_name = matches
                .iter()
                .find(|m| m.node_type == NodeType::KafkaTopic)
                .map(|m| m.name.as_str())
                .unwrap_or(query);
            output.push_str(&format!("\nSymbol: {} [KafkaTopic]\n", topic_name));

            let producers: Vec<_> = matches
                .iter()
                .filter(|m| m.role.as_deref() == Some("Producer"))
                .collect();
            let consumers: Vec<_> = matches
                .iter()
                .filter(|m| m.role.as_deref() == Some("Consumer"))
                .collect();
            let configs: Vec<_> = matches
                .iter()
                .filter(|m| {
                    m.role.as_deref() == Some("Configuration")
                        || m.role.as_deref() == Some("Definition")
                })
                .collect();
            let references: Vec<_> = matches
                .iter()
                .filter(|m| m.role.as_deref() == Some("Reference") || m.role.is_none())
                .collect();

            if !producers.is_empty() {
                output.push_str("\nProducers:\n");
                for p in producers {
                    output.push_str(&format!("  • {}:{}\n", p.path, p.line));
                    if let Some(prev) = &p.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !consumers.is_empty() {
                output.push_str("\nConsumers:\n");
                for c in consumers {
                    output.push_str(&format!("  • {}:{}\n", c.path, c.line));
                    if let Some(prev) = &c.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !configs.is_empty() {
                output.push_str("\nDefinitions & Configuration:\n");
                for cfg in configs {
                    output.push_str(&format!("  • {}:{}\n", cfg.path, cfg.line));
                    if let Some(prev) = &cfg.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }

            if !references.is_empty() {
                output.push_str("\nReferences:\n");
                for r in references {
                    output.push_str(&format!("  • {}:{}\n", r.path, r.line));
                    if let Some(prev) = &r.preview {
                        output.push_str(&format!("{}\n", prev.formatted));
                    }
                }
            }
        } else {
            for m in matches.iter().take(10) {
                output.push_str(&format!("\nMatch: {} [{:?}]\n", m.name, m.node_type));
                output.push_str(&format!("File: {}:{}\n", m.path, m.line));
                if let Some(role) = &m.role {
                    output.push_str(&format!("Role: {}\n", role));
                }
                if let Some(prev) = &m.preview {
                    output.push_str("Context:\n");
                    output.push_str(&format!("{}\n", prev.formatted));
                }
            }

            if matches.len() > 10 {
                output.push_str(&format!("\n... and {} more matches.\n", matches.len() - 10));
            }
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!(
                "\nNext Step: `project-map impact --fqn {}`",
                first.name
            ));
        }

        output
    }

    pub fn format_file_context(path: &str, symbols: &[NodeData]) -> String {
        let mut output = format!("Resource: FileContext | Path: {}\n", path);
        output.push_str("\n--- File Outline ---\n");

        if symbols.is_empty() {
            output.push_str("- (No symbols detected or file not indexed)\n");
        } else {
            for s in symbols {
                if s.role.as_deref() == Some("Import") {
                    output.push_str(&format!("- import {} ({})\n", s.name, s.path));
                } else {
                    output.push_str(&format!("- {} {} (line: {})\n", s.kind, s.name, s.line));
                }
            }
        }

        output.push_str(&format!(
            "\nNext Step: `project-map fetch --path {} --symbol <symbol_name>`",
            path
        ));
        output
    }

    pub fn format_impact_analysis(fqn: &str, impact: &[NodeData]) -> String {
        let mut output = format!("Resource: Impact Analysis | Target: {}\n", fqn);
        output.push_str(&format!("Nodes Impacted: {}\n", impact.len()));

        for node in impact.iter().take(10) {
            output.push_str(&format!(
                "- {:?}: {} ({})\n",
                node.node_type, node.name, node.path
            ));
        }

        if impact.len() > 10 {
            output.push_str(&format!("... and {} more.\n", impact.len() - 10));
        }

        if let Some(first) = impact.first() {
            output.push_str(&format!(
                "\nNext Step: `project-map blast --path {} --symbol {}` to see what this impacts.",
                first.path, first.name
            ));
        }

        output
    }

    pub fn format_blast_radius(path: &str, symbol: &str, results: &[NodeData]) -> String {
        let mut output = format!("Resource: Blast Radius | Symbol: {} in {}\n", symbol, path);

        if results.is_empty() {
            output.push_str("No dependent components found.\n");
        } else {
            let mut dir_counts: HashMap<String, usize> = HashMap::new();
            let mut unique_files: HashSet<String> = HashSet::new();

            for r in results {
                unique_files.insert(r.path.clone());
                let dir = std::path::Path::new(&r.path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("root")
                    .to_string();
                *dir_counts.entry(dir).or_insert(0) += 1;
            }

            output.push_str("Summary:\n");
            output.push_str(&format!("- Total Impacted Nodes: {}\n", results.len()));
            output.push_str(&format!(
                "- Unique Files Affected: {}\n",
                unique_files.len()
            ));
            output.push_str("- Affected Modules/Packages:\n");

            let mut sorted_dirs: Vec<_> = dir_counts.into_iter().collect();
            sorted_dirs.sort_by_key(|b| std::cmp::Reverse(b.1));

            for (dir, count) in sorted_dirs.iter().take(5) {
                output.push_str(&format!("  * {}: {} nodes\n", dir, count));
            }

            if sorted_dirs.len() > 5 {
                output.push_str(&format!(
                    "  * ... and {} more directories.\n",
                    sorted_dirs.len() - 5
                ));
            }

            output.push_str("\nTop Direct Dependents:\n");
            for r in results.iter().take(10) {
                output.push_str(&format!("- {} (ln: {}) -> {}\n", r.path, r.line, r.name));
            }

            if results.len() > 10 {
                output.push_str(&format!(
                    "... and {} more nodes omitted for brevity.\n",
                    results.len() - 10
                ));
            }

            if let Some(first) = results.first() {
                output.push_str(&format!(
                    "\nNext Step: `project-map fetch --path {} --symbol {}` to view source.",
                    first.path, first.name
                ));
            }
        }

        output
    }

    pub fn format_full_status(
        engine_opt: Option<&QueryEngine>,
        server_root: Option<&Path>,
        out_dir: Option<&Path>,
        index_path: Option<&str>,
        pulse_tree: Option<&str>,
        active_features: &[String],
    ) -> String {
        let mut output = "Project Map CLI - Status\n".to_string();
        if let Some(engine) = engine_opt {
            output.push_str("Phase: Ready\n");
            if let Some(path) = index_path {
                output.push_str(&format!("Index: Found ({})\n", path));
            }

            let file_count = engine.get_file_count();
            let symbol_count = engine.get_symbol_count();
            let edge_count = engine.get_edge_count();

            if let Some(meta) = engine.meta() {
                output.push_str(&format!("Tool Version: {}\n", meta.tool_version));
                output.push_str(&format!("Root: {}\n", meta.root));
                output.push_str(&format!("Built At: {}\n", meta.built_at));
                output.push_str(&format!("Files: {}\n", meta.file_count));
                output.push_str(&format!("Symbols: {}\n", meta.symbol_count));
                output.push_str(&format!("Edges: {}\n", meta.edge_count));
                // A schema-2 index has no doc counts; the stale line below says so.
                if !engine.is_stale() {
                    output.push_str(&format!("Doc Files: {}\n", meta.doc_file_count));
                    output.push_str(&format!("Doc Sections: {}\n", meta.section_count));
                    output.push_str(&format!("Doc References: {}\n", meta.reference_count));
                    output.push_str(&format!(
                        "Unresolved Mentions: {}\n",
                        meta.unresolved_mention_count
                    ));
                    output.push_str(&format!(
                        "Ambiguous Mentions: {}\n",
                        meta.ambiguous_mention_count
                    ));
                    output.push_str(&format!("API Operations: {}\n", meta.api_operation_count));
                }
            } else {
                output.push_str(&format!("Files: {}\n", file_count));
                output.push_str(&format!("Symbols: {}\n", symbol_count));
                output.push_str(&format!("Edges: {}\n", edge_count));
                output.push_str("WARNING: Legacy index without metadata block.\n");
            }
            if engine.is_stale() {
                output.push_str(STALE_INDEX_MESSAGE);
                output.push('\n');
            }

            // Warnings checks
            let all_paths = engine.get_all_file_paths();
            let out_canonical = out_dir.and_then(|d| d.canonicalize().ok());
            let is_out_dir_only = if all_paths.is_empty() {
                false
            } else {
                all_paths.iter().all(|p| {
                    p.starts_with(".project-map")
                        || p.starts_with("./.project-map")
                        || out_canonical.as_ref().is_some_and(|can_out| {
                            let can_p = Path::new(p).canonicalize().ok();
                            can_p.as_ref().is_some_and(|cp| cp.starts_with(can_out))
                        })
                })
            };

            if file_count == 0 {
                output.push_str("WARNING: Index contains 0 indexed files (empty index).\n");
            } else if is_out_dir_only {
                output.push_str(
                    "WARNING: Index contains only output directory files (degenerate index).\n",
                );
            }

            if let (Some(meta), Some(s_root)) = (engine.meta(), server_root) {
                if let Ok(can_srv) = s_root.canonicalize() {
                    let can_srv_str = can_srv.to_string_lossy().to_string();
                    if meta.root != can_srv_str {
                        output.push_str(&format!(
                            "WARNING: Index root ({}) differs from server root ({}).\n",
                            meta.root, can_srv_str
                        ));
                    }
                }
            }

            if let Some(tree) = pulse_tree {
                output.push_str("\n--- Project Pulse ---\n");
                output.push_str(tree);
            }

            if !active_features.is_empty() {
                output.push_str("\n--- Active Features ---\n");
                for feature in active_features {
                    output.push_str(&format!("- projects/active/{}\n", feature));
                }
            }
        } else {
            output.push_str("Phase: Discovery (No index found)\n");
            output.push_str("Next Step: Run `project-map build` to generate the index.\n");
        }
        output
    }

    pub fn format_status(
        is_ready: bool,
        index_path: Option<&str>,
        pulse_tree: Option<&str>,
        active_features: &[String],
    ) -> String {
        let mut output = "Project Map CLI - Status\n".to_string();
        if is_ready {
            output.push_str("Phase: Ready\n");
            if let Some(path) = index_path {
                output.push_str(&format!("Index: Found ({})\n", path));
            }

            if let Some(tree) = pulse_tree {
                if tree.contains(".project-map")
                    && !tree.contains(".rs")
                    && !tree.contains(".py")
                    && !tree.contains(".ts")
                    && !tree.contains(".js")
                {
                    output.push_str(
                        "WARNING: Index contains only output directory files (degenerate index).\n",
                    );
                }
                output.push_str("\n--- Project Pulse ---\n");
                output.push_str(tree);
            }

            if !active_features.is_empty() {
                output.push_str("\n--- Active Features ---\n");
                for feature in active_features {
                    output.push_str(&format!("- projects/active/{}\n", feature));
                }
            }
        } else {
            output.push_str("Phase: Discovery (No index found)\n");
            output.push_str("Next Step: Run `project-map build` to generate the index.\n");
        }
        output
    }

    pub fn format_file_matches(query: &str, matches: &[NodeData]) -> String {
        let mut output = format!("Resource: Files | Query: {}\n", query);
        output.push_str(&format!("Matches Found: {}\n", matches.len()));

        for m in matches.iter().take(10) {
            output.push_str(&format!("- {}\n", m.path));
        }

        if matches.len() > 10 {
            output.push_str(&format!("... and {} more.\n", matches.len() - 10));
        }

        if let Some(first) = matches.first() {
            output.push_str(&format!(
                "\nNext Step: `project-map context --path {}` to see the file outline.",
                first.path
            ));
        }

        output
    }

    /// Ranked doc sections: `- path#anchor | Heading` with up to two snippet lines.
    pub fn format_doc_search(query: &str, result: &DocSearch, max_chars: usize) -> String {
        let mut out = format!("Resource: Docs | Query: {}\n", query);
        if result.total > result.hits.len() {
            out.push_str(&format!(
                "Matches Found: {} (showing {})\n",
                result.total,
                result.hits.len()
            ));
        } else {
            out.push_str(&format!("Matches Found: {}\n", result.total));
        }
        for hit in &result.hits {
            out.push_str(&format!(
                "- {}#{} | {}\n",
                hit.path, hit.anchor, hit.heading
            ));
            for line in &hit.snippet {
                out.push_str(&format!("    {}\n", line));
            }
        }
        if let Some(first) = result.hits.first() {
            out.push_str(&format!(
                "\nNext Step: `doc_section` with path {} and anchor {} for the full text.",
                first.path, first.anchor
            ));
        }
        truncate_output(&out, max_chars)
    }

    /// Frontmatter on one line, then the heading tree: `heading #anchor`, two spaces per depth.
    pub fn format_doc_outline(view: &DocOutlineView, max_chars: usize) -> String {
        let mut out = format!("Resource: DocOutline | Path: {}\n", view.path);
        if let Some(meta) = &view.meta {
            let mut fields = Vec::new();
            for (key, value) in [
                ("id", &meta.id),
                ("title", &meta.title),
                ("status", &meta.status),
                ("date", &meta.date),
                ("domain", &meta.domain),
            ] {
                if let Some(v) = value {
                    fields.push(format!("{key}={v}"));
                }
            }
            if !meta.tags.is_empty() {
                fields.push(format!("tags={}", meta.tags.join(",")));
            }
            if !meta.supersedes.is_empty() {
                fields.push(format!("supersedes={}", meta.supersedes.join(",")));
            }
            out.push_str(&format!("Frontmatter: {}\n", fields.join(" | ")));
        }
        if view.entries.is_empty() {
            out.push_str("(no headings)\n");
        }
        for e in &view.entries {
            out.push_str(&format!(
                "{}{} #{}\n",
                "  ".repeat(e.depth),
                e.heading,
                e.anchor
            ));
        }
        truncate_output(out.trim_end(), max_chars)
    }

    /// The section text between `---` lines; truncation applies to the text only.
    pub fn format_doc_section(view: &DocSectionView, max_chars: usize) -> String {
        format!(
            "Resource: DocSection | Path: {} | Anchor: {}\n---\n{}\n---",
            view.path,
            view.anchor,
            truncate_output(&view.text, max_chars)
        )
    }

    pub fn format_doc_links(view: &DocLinksView, max_chars: usize) -> String {
        let mut out = String::new();
        match view {
            DocLinksView::CodeToDocs { path, symbol, refs } => {
                out.push_str(&format!("Resource: DocLinks | Path: {path}"));
                if let Some(symbol) = symbol {
                    out.push_str(&format!(" | Symbol: {symbol}"));
                }
                out.push('\n');
                if refs.is_empty() {
                    out.push_str("No documentation references this.\n");
                }
                for r in refs {
                    let id = format!("{}#{}", r.doc_path, r.anchor);
                    // Without a symbol filter, name what was referenced when it is not the file.
                    let via = if symbol.is_none() && r.target != *path {
                        format!(" -> {}", r.target.rsplit("::").next().unwrap_or(&r.target))
                    } else {
                        String::new()
                    };
                    out.push_str(&format!("- {id} | {}{via}\n", r.heading));
                }
            }
            DocLinksView::DocsToCode { path, sections } => {
                out.push_str(&format!("Resource: DocLinks | Path: {path}\n"));
                if sections.is_empty() {
                    out.push_str("This document references no indexed files or symbols.\n");
                }
                for s in sections {
                    out.push_str(&format!("- #{} | {}\n", s.anchor, s.heading));
                    for t in &s.targets {
                        out.push_str(&format!("    {t}\n"));
                    }
                }
            }
        }
        truncate_output(out.trim_end(), max_chars)
    }

    /// `- Term (count) path#anchor, ...`, most frequent first.
    pub fn format_doc_terms(view: &DocTermsView, max_chars: usize) -> String {
        let mut out = format!("Resource: DocTerms | Prefix: {}\n", view.prefix);
        if view.total > view.terms.len() {
            out.push_str(&format!(
                "Terms: {} (showing {})\n",
                view.total,
                view.terms.len()
            ));
        } else {
            out.push_str(&format!("Terms: {}\n", view.total));
        }
        for t in &view.terms {
            let at: Vec<_> = t.sections.iter().map(|(p, a)| format!("{p}#{a}")).collect();
            out.push_str(&format!("- {} ({}) {}\n", t.term, t.count, at.join(", ")));
        }
        truncate_output(out.trim_end(), max_chars)
    }

    /// Per file: `METHOD /path (operationId) - summary` lines, then the schema names.
    pub fn format_api_surface(files: &[ApiFileView], max_chars: usize) -> String {
        let mut out = format!("Resource: ApiSurface | Files: {}\n", files.len());
        for f in files {
            out.push_str(&format!("{}\n", f.path));
            for op in &f.operations {
                out.push_str(&format!("  {}", op.name));
                if let Some(id) = &op.operation_id {
                    out.push_str(&format!(" ({id})"));
                }
                if let Some(summary) = &op.summary {
                    out.push_str(&format!(" - {summary}"));
                }
                out.push('\n');
            }
            if !f.schemas.is_empty() {
                out.push_str(&format!("  schemas: {}\n", f.schemas.join(", ")));
            }
        }
        truncate_output(out.trim_end(), max_chars)
    }

    pub fn format_fetch_result(path: &str, symbol: &str, content: Option<&str>) -> String {
        if let Some(c) = content {
            format!(
                "Resource: Fetch | Path: {} | Symbol: {}\n---\n{}\n---",
                path, symbol, c
            )
        } else {
            format!(
                "Resource: Fetch | Status: Symbol not found: {} in {}",
                symbol, path
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_under_budget_is_unchanged() {
        let text = "line one\nline two\n";
        assert_eq!(truncate_output(text, 100), text);
        // Exactly at the budget is still under it.
        assert_eq!(truncate_output(text, text.chars().count()), text);
        assert_eq!(truncate_output("", 0), "");
    }

    #[test]
    fn test_truncate_over_budget_cuts_on_a_line_and_adds_one_marker() {
        let text = "alpha\nbeta\ngamma\ndelta\n";
        let out = truncate_output(text, 13);
        assert_eq!(
            out,
            "alpha\nbeta\n[truncated at 13 characters: raise max_chars or narrow the query to see more]"
        );
        assert_eq!(out.matches("[truncated").count(), 1);
        assert!(!out.contains("gamma"));
    }

    #[test]
    fn test_truncate_without_a_line_break_cuts_at_the_budget() {
        let out = truncate_output("abcdefghij", 4);
        assert!(out.starts_with("abcd\n[truncated at 4 characters"));
        // A leading line break is not a cut point: the start is kept, not nothing.
        let out = truncate_output("\nabcdefghij", 4);
        assert!(out.starts_with("\nabc\n[truncated"));
    }

    #[test]
    fn test_truncate_never_splits_a_multibyte_character() {
        // Each of these is 2-4 bytes; every budget from 0 to 12 must slice cleanly.
        let text = "é€🚀é€🚀é€🚀é€🚀";
        for max in 0..=text.chars().count() + 1 {
            let out = truncate_output(text, max);
            let body = out.split("\n[truncated").next().unwrap();
            assert!(body.chars().count() <= max, "budget {max}: {out}");
            assert!(text.starts_with(body));
        }
        let out = truncate_output(text, 4);
        assert!(out.starts_with("é€🚀é\n[truncated"));
    }
}
