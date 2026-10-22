use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use std::path::Path;
use tempfile::{tempdir, TempDir};

/// An engine over a copy of the docs fixture. The copy is built with an absolute
/// root, so indexed paths are root-relative (`docs/guide.md`) and the engine reads
/// text from the copy. Keep the returned `TempDir` alive; edit files in `root()`.
pub struct Fixture {
    pub dir: TempDir,
    pub index: std::path::PathBuf,
    pub engine: QueryEngine,
}

impl Fixture {
    pub fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("project")
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn fixture() -> Fixture {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_dir(Path::new("tests/fixtures/docs"), &root);
    let root = root.canonicalize().unwrap();
    let out = dir.path().join("out");
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &out).unwrap();
    let index = dir.path().join("index.json");
    orch.save_index(&index).unwrap();
    let engine = QueryEngine::load(&index).unwrap();
    Fixture { dir, index, engine }
}

/// A one-file project with the given Markdown, built with an absolute root.
pub fn project_with(markdown: &[(&str, &str)]) -> Fixture {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in markdown {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &dir.path().join("out")).unwrap();
    let index = dir.path().join("index.json");
    orch.save_index(&index).unwrap();
    let engine = QueryEngine::load(&index).unwrap();
    Fixture { dir, index, engine }
}

fn hit_ids(fx: &Fixture, query: &str, limit: usize) -> Vec<String> {
    fx.engine
        .search_docs(query, limit)
        .unwrap()
        .hits
        .iter()
        .map(|h| format!("{}#{}", h.path, h.anchor))
        .collect()
}

#[test]
fn search_docs_audit_log_returns_the_expected_sections_in_order() {
    let fx = fixture();
    let result = fx.engine.search_docs("audit log", 10).unwrap();
    assert_eq!(result.total, 6);
    let ids: Vec<_> = result
        .hits
        .iter()
        .map(|h| format!("{}#{}", h.path, h.anchor))
        .collect();
    assert_eq!(
        ids,
        [
            // Heading plus frontmatter id, title and tags.
            "docs/adr/004-immutable-audit-log.md#adr-004-audit-logs-are-immutable",
            "docs/guide.md#step-2-configure-auditlog-fast-",
            "docs/links.md#markdown-links",
            "docs/adr/004-immutable-audit-log.md#decision",
            "docs/links.md#qualified-symbols",
            "docs/links.md#fenced-code",
        ]
    );

    let step2 = &result.hits[1];
    assert_eq!(step2.heading, "Step 2: Configure AuditLog (fast!) 🚀");
    assert_eq!(
        step2.snippet[0],
        "See the [Audit Service](../src/audit/log.rs) source. A Release Manager appears only once."
    );
    assert_eq!(step2.snippet.len(), 2);

    // The snippet starts at the first line that mentions a keyword.
    let decision = &result.hits[3];
    assert_eq!(
        decision.snippet,
        [
            "`AuditLog::append` is the only writer, implemented in `src/audit/log.rs::append`.",
            "Rotation uses `rotate_segments`."
        ]
    );
}

#[test]
fn search_docs_honours_limit_and_reports_the_total() {
    let fx = fixture();
    let result = fx.engine.search_docs("audit log", 2).unwrap();
    assert_eq!((result.hits.len(), result.total), (2, 6));
    assert!(fx
        .engine
        .search_docs("audit log", 0)
        .unwrap()
        .hits
        .is_empty());
    assert!(fx.engine.search_docs("   ", 5).unwrap().hits.is_empty());
    assert_eq!(fx.engine.search_docs("zzzznothing", 5).unwrap().total, 0);
}

#[test]
fn search_docs_frontmatter_only_match_surfaces_the_first_section() {
    let fx = project_with(&[(
        "notes/handbook.md",
        "---\nid: HB-9\ntitle: Zebra Handbook\ntags: [giraffe]\n---\n\n# Intro\n\nNothing relevant.\n\n## Later\n\nStill nothing.\n",
    )]);
    for query in ["giraffe", "zebra", "hb-9", "giraffe zebra"] {
        assert_eq!(
            hit_ids(&fx, query, 5),
            ["notes/handbook.md#intro"],
            "{query}"
        );
    }
    // Other sections of the file do not inherit the frontmatter.
    assert!(fx
        .engine
        .search_docs("giraffe", 5)
        .unwrap()
        .hits
        .iter()
        .all(|h| h.anchor == "intro"));
}

#[test]
fn search_docs_survives_a_file_changed_after_indexing() {
    let fx = fixture();
    let guide = fx.root().join("docs/guide.md");
    // Shorter than every recorded range, and then multibyte text that puts the
    // recorded offsets inside characters.
    for replacement in ["x\n".to_string(), "🚀".repeat(400)] {
        std::fs::write(&guide, replacement).unwrap();
        let result = fx.engine.search_docs("audit log", 10).unwrap();
        // The heading still matches; the snippet is best effort.
        let step2 = result
            .hits
            .iter()
            .find(|h| h.anchor == "step-2-configure-auditlog-fast-")
            .expect("heading match survives");
        assert!(step2.snippet.len() <= 2);
    }
    // A deleted file does not crash either.
    std::fs::remove_file(&guide).unwrap();
    assert!(fx.engine.search_docs("audit log", 10).is_ok());
}

#[test]
fn search_docs_formats_hits_and_truncates() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let result = fx.engine.search_docs("audit log", 3).unwrap();
    let text = ToonFormatter::format_doc_search("audit log", &result, 4000);
    assert!(text.starts_with("Resource: Docs | Query: audit log\nMatches Found: 6 (showing 3)\n- docs/adr/004-immutable-audit-log.md#adr-004-audit-logs-are-immutable | ADR-004: Audit logs are immutable\n"));
    assert!(text.contains("\n    See the [Audit Service]"));
    assert!(!text.contains("[truncated"));

    let short = ToonFormatter::format_doc_search("audit log", &result, 120);
    assert!(short.contains("[truncated at 120 characters"));
}

#[test]
fn code_search_ignores_documentation() {
    let fx = fixture();
    // `Usage` and `Decision` are only headings; `append` is code and also in docs.
    assert!(fx.engine.find_symbols("Usage").is_empty());
    assert!(fx.engine.find_symbols("Decision").is_empty());
    assert!(fx.engine.find_symbols("GET /entries").is_empty());
    let append = fx.engine.find_symbols("append");
    assert!(!append.is_empty() && append.iter().all(|n| !n.node_type.is_doc()));
    // Docs mentioning a symbol are not code dependents.
    let radius = fx.engine.check_blast_radius("src/audit/log.rs", "append");
    assert!(
        radius.iter().all(|n| !n.path.ends_with(".md")),
        "{radius:?}"
    );
}

const ADR_004: &str = "docs/adr/004-immutable-audit-log.md";

#[test]
fn doc_outline_shows_frontmatter_and_the_nested_tree() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx.engine.doc_outline(ADR_004).unwrap();
    assert_eq!(view.path, ADR_004);
    let meta = view.meta.as_ref().unwrap();
    assert_eq!(meta.id.as_deref(), Some("ADR-004"));
    assert_eq!(meta.tags, ["audit", "compliance"]);

    assert_eq!(
        ToonFormatter::format_doc_outline(&view, 4000),
        "Resource: DocOutline | Path: docs/adr/004-immutable-audit-log.md\n\
         Frontmatter: id=ADR-004 | title=Audit logs are immutable | status=accepted | date=2026-10-01 | domain=audit | tags=audit,compliance | supersedes=ADR-001\n\
         ADR-004: Audit logs are immutable #adr-004-audit-logs-are-immutable\n\
         \x20\x20Context #context\n\
         \x20\x20Decision #decision\n\
         \x20\x20Consequences #consequences"
    );
}

#[test]
fn doc_outline_indents_by_tree_depth() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx.engine.doc_outline("docs/guide.md").unwrap();
    let text = ToonFormatter::format_doc_outline(&view, 4000);
    assert!(view.meta.is_none() && !text.contains("Frontmatter"));
    let lines: Vec<_> = text.lines().skip(1).collect();
    assert_eq!(lines[0], "Operator Guide #operator-guide");
    assert_eq!(lines[1], "  Usage #usage");
    // Duplicate headings keep their GitHub-style anchors.
    assert_eq!(lines[3], "  Usage 1 #usage-1-1");
    assert_eq!(lines[5], "    Skipped Level Target #skipped-level-target");
    assert_eq!(lines[6], "      Deep Detail #deep-detail");
    // An h3 directly under an h1 is one level deep.
    assert_eq!(lines.last().unwrap(), &"  Skipped From H1 #skipped-from-h1");
    assert_eq!(lines.len(), 12);
}

#[test]
fn doc_outline_path_variants_are_identical() {
    let fx = fixture();
    let plain = fx.engine.doc_outline(ADR_004).unwrap();
    let dotted = fx.engine.doc_outline(&format!("./{ADR_004}")).unwrap();
    let absolute = fx
        .engine
        .doc_outline(
            fx.root()
                .canonicalize()
                .unwrap()
                .join(ADR_004)
                .to_str()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(plain, dotted);
    assert_eq!(plain, absolute);
}

#[test]
fn doc_outline_unknown_and_non_markdown_paths_are_clear_errors() {
    let fx = fixture();
    for path in [
        "docs/nope.md",
        "src/audit/log.rs",
        "api/openapi.yaml",
        "docs",
    ] {
        let err = fx.engine.doc_outline(path).unwrap_err();
        assert!(
            err.starts_with(&format!(
                "Unknown document: {path} is not an indexed Markdown file"
            )),
            "{path}: {err}"
        );
    }
}

#[test]
fn doc_outline_of_a_file_without_headings_is_empty_not_an_error() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = project_with(&[("plain.md", "Just text.\n")]);
    let view = fx.engine.doc_outline("plain.md").unwrap();
    assert!(view.entries.is_empty());
    assert_eq!(
        ToonFormatter::format_doc_outline(&view, 4000),
        "Resource: DocOutline | Path: plain.md\n(no headings)"
    );
}

#[test]
fn doc_section_returns_the_exact_text() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx.engine.doc_section(ADR_004, "decision").unwrap();
    assert_eq!(view.heading, "Decision");
    assert!(view
        .text
        .starts_with("## Decision\n\n`AuditLog::append` is the only writer"));
    assert!(view.text.ends_with("Rotation uses `rotate_segments`."));
    // It stops before the next heading.
    assert!(!view.text.contains("## Consequences"));
    // A leading `#` and `./` path forms are accepted.
    assert_eq!(
        fx.engine
            .doc_section(&format!("./{ADR_004}"), "#decision")
            .unwrap(),
        view
    );

    let framed = ToonFormatter::format_doc_section(&view, 4000);
    assert!(framed.starts_with(&format!(
        "Resource: DocSection | Path: {ADR_004} | Anchor: decision\n---\n## Decision\n"
    )));
    assert!(framed.ends_with("`rotate_segments`.\n---"));
}

#[test]
fn doc_section_of_a_parent_includes_its_subsections() {
    let fx = fixture();
    let parent = fx
        .engine
        .doc_section("docs/guide.md", "usage-2")
        .unwrap()
        .text;
    assert!(parent.contains("Third plain duplicate."));
    assert!(parent.contains("### Skipped Level Target"));
    assert!(parent.contains("#### Deep Detail"));
    assert!(parent.contains("The Compliance Officer signs off."));
    // The next h2 is outside it.
    assert!(!parent.contains("Configure"));
    // The child alone is shorter and starts at its own heading.
    let child = fx
        .engine
        .doc_section("docs/guide.md", "skipped-level-target")
        .unwrap()
        .text;
    assert!(child.starts_with("### Skipped Level Target") && child.len() < parent.len());
}

#[test]
fn doc_section_truncates_with_the_marker() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx
        .engine
        .doc_section("docs/guide.md", "operator-guide")
        .unwrap();
    let framed = ToonFormatter::format_doc_section(&view, 60);
    assert!(framed.contains("[truncated at 60 characters"));
    assert_eq!(framed.matches("[truncated").count(), 1);
    assert!(framed.ends_with("\n---"));
}

#[test]
fn doc_section_unknown_anchor_lists_the_anchors() {
    let fx = fixture();
    let err = fx.engine.doc_section(ADR_004, "nope").unwrap_err();
    assert_eq!(
        err,
        "Unknown anchor #nope in docs/adr/004-immutable-audit-log.md. Anchors: \
         adr-004-audit-logs-are-immutable, context, decision, consequences"
    );
    // A long list is capped.
    let mut doc = String::new();
    for n in 0..40 {
        doc.push_str(&format!("# H{n}\n\n"));
    }
    let big = project_with(&[("big.md", doc.as_str())]);
    let err = big.engine.doc_section("big.md", "zzz").unwrap_err();
    assert!(err.ends_with("... and 10 more"), "{err}");
    assert!(err.contains("h29") && !err.contains("h30"));
    assert!(fx
        .engine
        .doc_section("docs/nope.md", "x")
        .unwrap_err()
        .starts_with("Unknown document"));
}

#[test]
fn doc_section_after_the_file_was_edited_reports_a_stale_range() {
    let fx = fixture();
    let adr = fx.root().join(ADR_004);
    let original = std::fs::read_to_string(&adr).unwrap();
    let stale = |fx: &Fixture| fx.engine.doc_section(ADR_004, "decision").unwrap_err();

    // Text inserted before the section shifts it: the range no longer starts at a heading.
    std::fs::write(
        &adr,
        format!("A new paragraph at the top of the file.\n\n{original}"),
    )
    .unwrap();
    assert!(stale(&fx).starts_with(
        "Stale range: docs/adr/004-immutable-audit-log.md changed since it was indexed"
    ));
    // Shorter than the recorded range.
    std::fs::write(&adr, "# short\n").unwrap();
    assert!(stale(&fx).contains("Run `refresh`"));
    // Multibyte text: the offsets land inside characters.
    std::fs::write(&adr, "🚀".repeat(500)).unwrap();
    assert!(stale(&fx).starts_with("Stale range"));
    // Deleted.
    std::fs::remove_file(&adr).unwrap();
    assert!(stale(&fx).starts_with("Stale range"));
    // Unedited files still read.
    assert!(fx.engine.doc_section("docs/guide.md", "usage").is_ok());
}

#[test]
fn doc_links_code_to_docs_for_a_symbol() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx
        .engine
        .doc_links("src/audit/log.rs", Some("append"))
        .unwrap();
    assert_eq!(
        ToonFormatter::format_doc_links(&view, 4000),
        "Resource: DocLinks | Path: src/audit/log.rs | Symbol: append\n\
         - docs/adr/004-immutable-audit-log.md#decision | Decision\n\
         - docs/links.md#qualified-symbols | Qualified Symbols"
    );
}

#[test]
fn doc_links_code_to_docs_for_a_whole_file_names_the_symbols() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let text = ToonFormatter::format_doc_links(
        &fx.engine.doc_links("./src/audit/log.rs", None).unwrap(),
        4000,
    );
    assert_eq!(
        text,
        "Resource: DocLinks | Path: src/audit/log.rs\n\
         - docs/adr/004-immutable-audit-log.md#decision | Decision -> append\n\
         - docs/adr/004-immutable-audit-log.md#decision | Decision -> rotate_segments\n\
         - docs/guide.md#step-2-configure-auditlog-fast- | Step 2: Configure AuditLog (fast!) 🚀\n\
         - docs/guide.md#step-2-configure-auditlog-fast- | Step 2: Configure AuditLog (fast!) 🚀 -> AuditLog\n\
         - docs/links.md# | (before first heading)\n\
         - docs/links.md#markdown-links | Markdown Links\n\
         - docs/links.md#qualified-symbols | Qualified Symbols\n\
         - docs/links.md#qualified-symbols | Qualified Symbols -> append\n\
         - docs/links.md#qualified-symbols | Qualified Symbols -> new\n\
         - docs/links.md#bare-names | Bare Names -> rotate_segments\n\
         - docs/links.md#fenced-code | Fenced Code -> new\n\
         - docs/links.md#fenced-code | Fenced Code -> verify_chain"
    );
}

#[test]
fn doc_links_docs_to_code_for_an_adr() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx.engine.doc_links(ADR_004, None).unwrap();
    assert_eq!(
        ToonFormatter::format_doc_links(&view, 4000),
        "Resource: DocLinks | Path: docs/adr/004-immutable-audit-log.md\n\
         - #decision | Decision\n\
         \x20\x20\x20\x20src/audit/log.rs::append\n\
         \x20\x20\x20\x20src/audit/log.rs::rotate_segments"
    );
    // A document mentioning a file links to the file itself.
    let text = ToonFormatter::format_doc_links(
        &fx.engine
            .doc_links("docs/adr/005-retention.md", None)
            .unwrap(),
        4000,
    );
    assert!(
        text.ends_with("- #decision | Decision\n    src/audit/retention.rs"),
        "{text}"
    );
}

#[test]
fn doc_links_with_no_links_is_an_explicit_empty_result() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let none = fx.engine.doc_links("config/settings.yaml", None).unwrap();
    assert_eq!(
        ToonFormatter::format_doc_links(&none, 4000),
        "Resource: DocLinks | Path: config/settings.yaml\nNo documentation references this."
    );
    let quiet = project_with(&[("quiet.md", "# Quiet\n\nNothing linked.\n")]);
    let view = quiet.engine.doc_links("quiet.md", None).unwrap();
    assert_eq!(
        ToonFormatter::format_doc_links(&view, 4000),
        "Resource: DocLinks | Path: quiet.md\nThis document references no indexed files or symbols."
    );
}

#[test]
fn doc_links_unknown_path_or_symbol_is_an_error() {
    let fx = fixture();
    assert_eq!(
        fx.engine.doc_links("src/missing.rs", None).unwrap_err(),
        "Unknown file: src/missing.rs is not indexed."
    );
    assert_eq!(
        fx.engine
            .doc_links("src/audit/log.rs", Some("nope"))
            .unwrap_err(),
        "Unknown symbol: nope in src/audit/log.rs."
    );
}

#[test]
fn doc_links_truncates() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let view = fx.engine.doc_links("src/audit/log.rs", None).unwrap();
    assert!(ToonFormatter::format_doc_links(&view, 200).contains("[truncated at 200 characters"));
}

#[test]
fn api_surface_lists_operations_and_schemas_for_yaml_and_json_specs() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let files = fx.engine.api_surface(None).unwrap();
    assert_eq!(
        ToonFormatter::format_api_surface(&files, 4000),
        "Resource: ApiSurface | Files: 3\n\
         api/billing-api.yml\n\
         \x20\x20PATCH /retention (setRetention) - Change the retention window\n\
         \x20\x20TRACE /retention - Trace retention\n\
         api/openapi.yaml\n\
         \x20\x20GET /entries (listEntries) - List audit entries\n\
         \x20\x20POST /entries (appendEntry) - Append an audit entry\n\
         \x20\x20GET /entries/{id} (getEntry)\n\
         \x20\x20schemas: AuditEntry, AuditPage\n\
         api/swagger.json\n\
         \x20\x20GET /invoices (listInvoices) - List invoices\n\
         \x20\x20PUT /invoices - Replace invoices\n\
         \x20\x20DELETE /invoices/{id} (deleteInvoice)\n\
         \x20\x20schemas: Invoice"
    );
}

#[test]
fn api_surface_for_one_path_and_unknown_paths() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = fixture();
    let one = fx.engine.api_surface(Some("./api/swagger.json")).unwrap();
    assert_eq!(one.len(), 1);
    assert!(ToonFormatter::format_api_surface(&one, 4000)
        .starts_with("Resource: ApiSurface | Files: 1\napi/swagger.json\n"));
    // A spec with no operations or schemas, a non-spec file and a missing file.
    for path in [
        "api/empty-openapi.json",
        "config/settings.yaml",
        "api/missing.yaml",
    ] {
        let err = fx.engine.api_surface(Some(path)).unwrap_err();
        assert_eq!(
            err,
            format!("No API operations or schemas are indexed for {path}.")
        );
    }
    let none = project_with(&[("a.md", "# A\n")]);
    assert_eq!(
        none.engine.api_surface(None).unwrap_err(),
        "No OpenAPI files are indexed."
    );
}

fn term_rows(fx: &Fixture, prefix: Option<&str>, limit: usize) -> Vec<(String, u32)> {
    fx.engine
        .doc_terms(prefix, limit)
        .unwrap()
        .terms
        .into_iter()
        .map(|t| (t.term, t.count))
        .collect()
}

#[test]
fn doc_terms_merges_directories_under_a_prefix() {
    // A term needs two occurrences in a directory to be stored at all, so
    // `Casting Director` (once in docs/deep) is dropped there before merging.
    let fx = project_with(&[
        ("docs/a.md", "# A\n\nCasting Director, Casting Director, Audit Service, Audit Service.\n"),
        ("docs/deep/b.md", "# B\n\nCasting Director and Audit Service and Audit Service.\n\nRetention Window, Retention Window.\n"),
        ("other/c.md", "# C\n\nCasting Director, Casting Director, Casting Director.\n"),
    ]);
    let row = |t: &str, c: u32| (t.to_string(), c);
    // Whole index: Casting Director 2 + 3, Audit Service 2 + 2, Retention Window 2.
    assert_eq!(
        term_rows(&fx, None, 30),
        [
            row("Casting Director", 5),
            row("Audit Service", 4),
            row("Retention Window", 2)
        ]
    );
    // `docs` includes `docs/deep`, not `other`; `./` and a trailing `/` are accepted.
    for prefix in ["docs", "./docs", "docs/"] {
        assert_eq!(
            term_rows(&fx, Some(prefix), 30),
            [
                row("Audit Service", 4),
                row("Casting Director", 2),
                row("Retention Window", 2)
            ],
            "{prefix}"
        );
    }
    // A prefix is a directory, not a string prefix.
    assert!(term_rows(&fx, Some("doc"), 30).is_empty());
    assert_eq!(
        term_rows(&fx, Some("docs/deep"), 30),
        [row("Audit Service", 2), row("Retention Window", 2)]
    );
    // An empty or dot prefix means everything.
    assert_eq!(term_rows(&fx, Some(""), 30), term_rows(&fx, None, 30));
    assert_eq!(term_rows(&fx, Some("."), 30), term_rows(&fx, None, 30));
}

#[test]
fn doc_terms_limit_ordering_and_sections() {
    use project_map_cli_rust::core::toon::ToonFormatter;
    let fx = project_with(&[
        (
            "docs/a.md",
            "# A\n\nBeta Two, Beta Two. Alpha One, Alpha One.\n\n## Sub\n\nAlpha One again.\n",
        ),
        ("docs/z/b.md", "# B\n\nAlpha One, Alpha One.\n"),
    ]);
    let view = fx.engine.doc_terms(None, 1).unwrap();
    assert_eq!((view.total, view.terms.len()), (2, 1));
    assert_eq!(view.terms[0].term, "Alpha One");
    assert_eq!(view.terms[0].count, 5);
    // Merged across directories: by path, then stored order within a file.
    assert_eq!(
        view.terms[0].sections,
        [
            ("docs/a.md".to_string(), "a".to_string()),
            ("docs/a.md".to_string(), "sub".to_string()),
            ("docs/z/b.md".to_string(), "b".to_string()),
        ]
    );
    assert_eq!(
        ToonFormatter::format_doc_terms(&view, 4000),
        "Resource: DocTerms | Prefix: .\nTerms: 2 (showing 1)\n- Alpha One (5) docs/a.md#a, docs/a.md#sub, docs/z/b.md#b"
    );
    assert!(term_rows(&fx, None, 0).is_empty());
}

#[test]
fn doc_terms_is_deterministic_across_calls_and_builds() {
    let a = fixture();
    let b = fixture();
    let first = a.engine.doc_terms(None, 30).unwrap();
    assert_eq!(first, a.engine.doc_terms(None, 30).unwrap());
    assert_eq!(first, b.engine.doc_terms(None, 30).unwrap());
    assert!(first.terms.iter().any(|t| t.term == "Casting Director"));
    assert!(first
        .terms
        .windows(2)
        .all(|w| (std::cmp::Reverse(w[0].count), &w[0].term)
            < (std::cmp::Reverse(w[1].count), &w[1].term)));
}

#[test]
fn every_doc_query_returns_the_stale_message_on_an_old_index_and_writes_nothing() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;
    for fixture_name in ["v0_2_0_index.json", "legacy_v0_1_18_index.json"] {
        let dir = tempdir().unwrap();
        let index = dir.path().join("index.json");
        std::fs::copy(
            Path::new("tests/fixtures/compatibility").join(fixture_name),
            &index,
        )
        .unwrap();
        let before = std::fs::read(&index).unwrap();
        let listing_before = std::fs::read_dir(dir.path()).unwrap().count();

        let engine = QueryEngine::load(&index).unwrap();
        assert!(engine.is_stale(), "{fixture_name}");
        let stale = |r: Result<(), String>| {
            assert_eq!(r.unwrap_err(), STALE_INDEX_MESSAGE, "{fixture_name}")
        };
        stale(engine.search_docs("audit", 10).map(drop));
        stale(engine.doc_outline("README.md").map(drop));
        stale(engine.doc_section("README.md", "intro").map(drop));
        stale(engine.doc_links("src/main.rs", None).map(drop));
        stale(engine.doc_terms(None, 30).map(drop));
        stale(engine.api_surface(None).map(drop));

        assert_eq!(
            std::fs::read(&index).unwrap(),
            before,
            "{fixture_name}: index bytes changed"
        );
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            listing_before,
            "{fixture_name}: files were written"
        );
    }
}

#[test]
fn a_current_index_does_not_return_the_stale_message() {
    use project_map_cli_rust::core::graph::STALE_INDEX_MESSAGE;
    let fx = fixture();
    assert!(!fx.engine.is_stale());
    let err = fx.engine.doc_outline("nope.md").unwrap_err();
    assert_ne!(err, STALE_INDEX_MESSAGE);
}

/// User story 2: the outline of every Markdown file plus the term list, for the
/// fixture, fit in 2,000 tokens (characters / 4), so an LLM can take the structure
/// and vocabulary of a docs tree cheaply.
#[test]
fn outlines_and_terms_for_the_fixture_stay_under_2000_tokens() {
    use project_map_cli_rust::core::toon::{ToonFormatter, DEFAULT_MAX_CHARS};

    const ROOT: &str = "tests/fixtures/docs";
    let dir = tempdir().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(Path::new(ROOT), &dir.path().join("out"))
        .unwrap();
    let index = dir.path().join("index.json");
    orch.save_index(&index).unwrap();
    let engine = QueryEngine::load(&index).unwrap();

    fn markdown_files(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                markdown_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path.to_string_lossy().into_owned());
            }
        }
    }
    let mut files = Vec::new();
    markdown_files(Path::new(ROOT), &mut files);
    files.sort();
    assert_eq!(files.len(), 5);

    let mut chars = 0;
    for file in &files {
        let view = engine.doc_outline(file).unwrap();
        let text = ToonFormatter::format_doc_outline(&view, DEFAULT_MAX_CHARS);
        assert!(!text.contains("[truncated"), "{file}");
        chars += text.chars().count();
    }
    let terms = ToonFormatter::format_doc_terms(
        &engine.doc_terms(Some(ROOT), 30).unwrap(),
        DEFAULT_MAX_CHARS,
    );
    assert!(terms.contains("Casting Director"));
    chars += terms.chars().count();

    let tokens = chars / 4;
    println!("outlines + terms: {chars} chars, ~{tokens} tokens");
    assert!(tokens < 2000, "{tokens} tokens");
}
