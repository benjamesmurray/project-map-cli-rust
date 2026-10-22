use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use project_map_cli_rust::core::graph::{EdgeType, NodeType, ProjectGraph};
use project_map_cli_rust::core::orchestrator::Orchestrator;
use std::path::Path;
use tempfile::tempdir;

const FIXTURE: &str = "tests/fixtures/docs";

/// Builds the docs fixture and reloads it from disk, as a query would see it.
/// A relative root keeps the root as a path prefix in the index.
fn build(root: &str) -> ProjectGraph {
    let out = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(root), out.path()).unwrap();
    let file = out.path().join("index.json");
    orch.save_index(&file).unwrap();
    ProjectGraph::load(&file).unwrap()
}

fn nodes_of(g: &ProjectGraph, ty: NodeType, path_suffix: &str) -> Vec<NodeIndex> {
    g.graph
        .node_indices()
        .filter(|&i| g.graph[i].node_type == ty && g.graph[i].path.ends_with(path_suffix))
        .collect()
}

fn children(g: &ProjectGraph, parent: NodeIndex) -> Vec<NodeIndex> {
    let mut kids: Vec<_> = g
        .graph
        .edges(parent)
        .filter(|e| *e.weight() == EdgeType::Contains)
        .map(|e| e.target())
        .collect();
    kids.sort_by_key(|&k| g.graph[k].start_byte);
    kids
}

/// Indented `name [anchor]` tree of the sections under a file node.
fn section_tree(g: &ProjectGraph, node: NodeIndex, depth: usize, out: &mut Vec<String>) {
    for kid in children(g, node) {
        let n = &g.graph[kid];
        out.push(format!(
            "{}{} [{}]",
            "  ".repeat(depth),
            n.name,
            n.role.clone().unwrap_or_default()
        ));
        section_tree(g, kid, depth + 1, out);
    }
}

fn file_node(g: &ProjectGraph, path_suffix: &str) -> NodeIndex {
    nodes_of(g, NodeType::File, path_suffix)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no file node ending {path_suffix}"))
}

#[test]
fn doc_pass_builds_section_tree_with_anchors() {
    let g = build(FIXTURE);
    let mut tree = Vec::new();
    section_tree(&g, file_node(&g, "docs/guide.md"), 0, &mut tree);
    assert_eq!(
        tree,
        [
            "Operator Guide [operator-guide]",
            "  Usage [usage]",
            "  Usage [usage-1]",
            "  Usage 1 [usage-1-1]",
            "  Usage [usage-2]",
            "    Skipped Level Target [skipped-level-target]",
            "      Deep Detail [deep-detail]",
            "  Step 2: Configure AuditLog (fast!) 🚀 [step-2-configure-auditlog-fast-]",
            "  Überblick & Café [überblick--café]",
            "  Setext Heading Level Two [setext-heading-level-two]",
            "Casting Director Notes [casting-director-notes]",
            "  Skipped From H1 [skipped-from-h1]",
        ]
    );
    let usage = &g.graph[children(&g, children(&g, file_node(&g, "docs/guide.md"))[0])[0]];
    assert_eq!(
        (usage.kind.as_str(), usage.node_type.clone()),
        ("h2", NodeType::DocSection)
    );
    assert!(usage.docstring.is_none());
    assert!(usage.end_byte > usage.start_byte && usage.line > 1);
}

#[test]
fn doc_pass_counts_sections_per_file() {
    let g = build(FIXTURE);
    let sections = |suffix: &str| nodes_of(&g, NodeType::DocSection, suffix).len();
    assert_eq!(sections("docs/guide.md"), 12);
    assert_eq!(sections("adr/004-immutable-audit-log.md"), 4);
    assert_eq!(sections("adr/005-retention.md"), 3);
    assert_eq!(sections("adr/006-malformed.md"), 1);
    // Non-Markdown files get no sections.
    assert_eq!(nodes_of(&g, NodeType::DocSection, ".yaml").len(), 0);
}

#[test]
fn doc_pass_stores_frontmatter_by_normalised_path() {
    let g = build(FIXTURE);
    let key = |suffix: &str| format!("{FIXTURE}/docs/adr/{suffix}");

    let m004 = &g.doc_meta[&key("004-immutable-audit-log.md")];
    assert_eq!(m004.id.as_deref(), Some("ADR-004"));
    assert_eq!(m004.tags, ["audit", "compliance"]);
    assert_eq!(m004.supersedes, ["ADR-001"]);
    assert_eq!(m004.date.as_deref(), Some("2026-10-01"));

    let m005 = &g.doc_meta[&key("005-retention.md")];
    assert_eq!(m005.id.as_deref(), Some("5"));
    assert_eq!(m005.tags, ["retention"]);
    assert_eq!(m005.supersedes, ["ADR-002", "ADR-003"]);

    // Malformed frontmatter: no DocMeta, sections still indexed.
    assert!(!g.doc_meta.contains_key(&key("006-malformed.md")));
    assert_eq!(g.doc_meta.len(), 2);
}

#[test]
fn doc_pass_adds_api_nodes_under_the_spec_file() {
    let g = build(FIXTURE);
    let ops = |suffix: &str| -> Vec<String> {
        let mut v: Vec<_> = nodes_of(&g, NodeType::ApiOperation, suffix)
            .into_iter()
            .map(|i| g.graph[i].name.clone())
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        ops("api/openapi.yaml"),
        ["GET /entries", "GET /entries/{id}", "POST /entries"]
    );
    assert_eq!(
        ops("api/swagger.json"),
        ["DELETE /invoices/{id}", "GET /invoices", "PUT /invoices"]
    );
    assert_eq!(
        ops("api/billing-api.yml"),
        ["PATCH /retention", "TRACE /retention"]
    );
    assert!(ops("api/empty-openapi.json").is_empty());
    assert!(ops("config/settings.yaml").is_empty());
    assert_eq!(nodes_of(&g, NodeType::ApiOperation, "").len(), 8);
    assert_eq!(nodes_of(&g, NodeType::ApiSchema, "").len(), 3);

    let get_entry = nodes_of(&g, NodeType::ApiOperation, "api/openapi.yaml")
        .into_iter()
        .map(|i| &g.graph[i])
        .find(|n| n.name == "GET /entries/{id}")
        .unwrap();
    assert_eq!(get_entry.role.as_deref(), Some("getEntry"));
    assert_eq!(get_entry.kind, "get");
    let list = nodes_of(&g, NodeType::ApiOperation, "api/openapi.yaml")
        .into_iter()
        .map(|i| &g.graph[i])
        .find(|n| n.name == "GET /entries")
        .unwrap();
    assert_eq!(list.docstring.as_deref(), Some("List audit entries"));

    // Contains edge from the spec file to each of its API nodes.
    let spec = file_node(&g, "api/openapi.yaml");
    assert_eq!(children(&g, spec).len(), 3 + 2);
}

#[test]
fn index_json_holds_no_section_body_text() {
    let out = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(FIXTURE), out.path()).unwrap();
    let file = out.path().join("index.json");
    orch.save_index(&file).unwrap();
    let json = std::fs::read_to_string(file).unwrap();
    // Body sentences from guide.md and an ADR; headings are fine, bodies are not.
    assert!(json.contains("Skipped Level Target"));
    assert!(!json.contains("Second section with the same heading"));
    assert!(!json.contains("Unicode heading for slug tests"));
    assert!(!json.contains("never changes"));
}

#[test]
fn doc_pass_gives_the_same_tree_for_an_absolute_root() {
    let abs = std::fs::canonicalize(FIXTURE).unwrap();
    let g = build(abs.to_str().unwrap());
    let mut tree = Vec::new();
    section_tree(&g, file_node(&g, "docs/guide.md"), 0, &mut tree);
    assert_eq!(tree.len(), 12);
    assert!(g
        .doc_meta
        .contains_key("docs/adr/004-immutable-audit-log.md"));
}

/// `doc#anchor -> target` for every link-pass edge (those starting at a doc file or
/// section), with the fixture prefix removed. Sorted.
fn doc_references(g: &ProjectGraph) -> Vec<String> {
    let strip = |p: &str| p.trim_start_matches("tests/fixtures/docs/").to_string();
    let mut rows: Vec<String> = g
        .graph
        .edge_references()
        .filter(|e| *e.weight() == EdgeType::References)
        .filter(|e| g.graph[e.source()].path.ends_with(".md"))
        .map(|e| {
            let (from, to) = (&g.graph[e.source()], &g.graph[e.target()]);
            let target = if to.node_type == NodeType::File {
                strip(&to.path)
            } else {
                format!("{}::{}", strip(&to.path), to.name)
            };
            format!(
                "{}#{} -> {}",
                strip(&from.path),
                from.role.clone().unwrap_or_default(),
                target
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn link_pass_adds_references_from_the_deepest_section_only() {
    let g = build(FIXTURE);
    let refs = doc_references(&g);
    let links: Vec<_> = refs
        .iter()
        .filter(|r| r.starts_with("docs/links.md"))
        .collect();
    assert_eq!(
        links,
        [
            // Text before the first heading: the edge starts at the file.
            "docs/links.md# -> src/audit/log.rs",
            "docs/links.md#bare-names -> src/audit/log.rs::rotate_segments",
            "docs/links.md#fenced-code -> src/audit/log.rs::new",
            "docs/links.md#fenced-code -> src/audit/log.rs::verify_chain",
            "docs/links.md#fenced-code -> src/audit/retention.rs",
            "docs/links.md#markdown-links -> docs/guide.md",
            "docs/links.md#markdown-links -> src/audit/log.rs",
            "docs/links.md#paths -> src/audit/retention.rs",
            "docs/links.md#paths -> src/billing/invoice.py",
            // `log.rs:12` and `AuditLog::append` both reach `append`: one edge.
            "docs/links.md#qualified-symbols -> src/audit/log.rs",
            "docs/links.md#qualified-symbols -> src/audit/log.rs::append",
            "docs/links.md#qualified-symbols -> src/audit/log.rs::new",
        ]
    );
    // No ancestor of a mentioning section carries an edge.
    for ancestor in ["link-coverage", "code-spans"] {
        assert!(
            !refs
                .iter()
                .any(|r| r.contains(&format!("links.md#{ancestor} "))),
            "{ancestor} must not carry a child's edge"
        );
    }
}

#[test]
fn link_pass_links_the_adr_and_guide_mentions() {
    let g = build(FIXTURE);
    let refs = doc_references(&g);
    let others: Vec<_> = refs
        .iter()
        .filter(|r| !r.starts_with("docs/links.md"))
        .collect();
    assert_eq!(
        others,
        [
            "docs/adr/004-immutable-audit-log.md#decision -> src/audit/log.rs::append",
            "docs/adr/004-immutable-audit-log.md#decision -> src/audit/log.rs::rotate_segments",
            "docs/adr/005-retention.md#decision -> src/audit/retention.rs",
            "docs/guide.md#step-2-configure-auditlog-fast- -> src/audit/log.rs",
            "docs/guide.md#step-2-configure-auditlog-fast- -> src/audit/log.rs::AuditLog",
        ]
    );
}

#[test]
fn link_pass_counts_match_the_hand_counted_fixture() {
    let meta = build(FIXTURE).meta.unwrap();
    // links.md: 12 edges; ADRs and the guide: 5.
    assert_eq!(meta.reference_count, 17);
    // links.md: Cargo.toml, missing.rs, nothing.rs, `new`, `run`, `does_not_exist`;
    // guide.md: the `Casting Director` code span gives two unknown bare names.
    // Unresolved bare names in fenced code (including the guide's) are not counted.
    assert_eq!(meta.unresolved_mention_count, 8);
    // links.md: RetentionPolicy::apply, flush_buffer; ADR-005: bare `RetentionPolicy`.
    assert_eq!(meta.ambiguous_mention_count, 3);
}

#[test]
fn link_pass_gives_the_same_edges_for_an_absolute_root() {
    let abs = std::fs::canonicalize(FIXTURE).unwrap();
    let g = build(abs.to_str().unwrap());
    let rel = doc_references(&build(FIXTURE));
    let strip = |rows: Vec<String>| -> Vec<String> {
        rows.into_iter()
            .map(|r| r.replace("tests/fixtures/docs/", ""))
            .collect()
    };
    assert_eq!(strip(doc_references(&g)), strip(rel));
    let meta = g.meta.unwrap();
    assert_eq!(
        (
            meta.reference_count,
            meta.unresolved_mention_count,
            meta.ambiguous_mention_count
        ),
        (17, 8, 3)
    );
}

#[test]
fn doc_terms_are_stored_per_directory() {
    let g = build(FIXTURE);
    let guide_dir = &g.doc_terms[&format!("{FIXTURE}/docs")];
    let casting = guide_dir
        .iter()
        .find(|t| t.term == "Casting Director")
        .unwrap();
    // Occurrences across guide.md and links.md, which share a directory.
    assert!(casting.count >= 3);
    assert!(casting.sections.len() <= 5);
    assert!(casting.sections.iter().all(|(p, _)| p.starts_with(FIXTURE)));
    // Once-only terms are dropped.
    assert!(guide_dir
        .iter()
        .all(|t| t.term != "Release Manager" && t.count >= 2));
    assert!(guide_dir.len() <= 50);
}

#[test]
fn doc_terms_are_byte_identical_across_builds() {
    let terms_json = || {
        let out = tempdir().unwrap();
        let mut orch = Orchestrator::new();
        orch.build_index(Path::new(FIXTURE), out.path()).unwrap();
        let file = out.path().join("index.json");
        orch.save_index(&file).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        serde_json::to_string(&v["doc_terms"]).unwrap()
    };
    let (first, second) = (terms_json(), terms_json());
    assert!(first.contains("Casting Director"));
    assert_eq!(first, second);
}

fn status_of(index: &Path) -> String {
    use project_map_cli_rust::core::query_engine::QueryEngine;
    use project_map_cli_rust::core::toon::ToonFormatter;
    let engine = QueryEngine::load(index).unwrap();
    ToonFormatter::format_full_status(Some(&engine), None, None, index.to_str(), None, &[])
}

#[test]
fn status_reports_doc_counts_for_a_current_index() {
    let out = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(FIXTURE), out.path()).unwrap();
    let file = out.path().join("index.json");
    orch.save_index(&file).unwrap();

    let status = status_of(&file);
    for line in [
        "Doc Files: 5",
        "Doc Sections: 27",
        "Doc References: 17",
        "Unresolved Mentions: 8",
        "Ambiguous Mentions: 3",
        "API Operations: 8",
    ] {
        assert!(status.contains(line), "missing `{line}` in:\n{status}");
    }
    assert!(!status.contains("STALE"));
}

#[test]
fn status_on_a_schema_2_index_shows_the_stale_line_not_doc_counts() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;
    let temp = tempdir().unwrap();
    let file = temp.path().join("index.json");
    std::fs::copy("tests/fixtures/compatibility/v0_2_0_index.json", &file).unwrap();

    let status = status_of(&file);
    assert!(status.contains(STALE_INDEX_MESSAGE), "{status}");
    for label in [
        "Doc Files",
        "Doc Sections",
        "Unresolved Mentions",
        "API Operations",
    ] {
        assert!(
            !status.contains(label),
            "{label} must not appear:\n{status}"
        );
    }
}
