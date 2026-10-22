//! Task 3.1: `status_result` with the Git state of the index against the tree now.

mod common;

use common::*;
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use project_map_cli_rust::core::results::{IndexState, StatusResult};
use project_map_cli_rust::core::toon::ToonFormatter;
use std::path::Path;
use tempfile::{tempdir, TempDir};

const DOCS: &str = "tests/fixtures/docs";

/// Builds an index of `root` (kept outside it, so the index does not dirty the tree).
fn engine_over(base: &Path, root: &Path) -> QueryEngine {
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &base.join("out")).unwrap();
    let index = base.join("index.json");
    orch.save_index(&index).unwrap();
    QueryEngine::load(&index).unwrap()
}

fn git_repo() -> (TempDir, String) {
    let dir = tempdir().unwrap();
    let sha = git_fixture(DOCS, &dir.path().join("repo"));
    (dir, sha)
}

fn toon(engine: &QueryEngine) -> String {
    ToonFormatter::format_full_status(Some(engine), None, None, None, None, &[])
}

#[test]
fn clean_tree_at_the_indexed_commit() {
    let (dir, sha) = git_repo();
    let engine = engine_over(dir.path(), &dir.path().join("repo"));
    let status = engine.status_result(3, vec![]);
    assert_eq!(status.commit.as_deref(), Some(sha.as_str()));
    assert_eq!(status.dirty, Some(false));
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.head_now.as_deref(), Some(sha.as_str()));
    assert_eq!(status.dirty_now, Some(false));
    assert_eq!(status.at_head, Some(true));
    assert_eq!(status.git_error, None);

    let text = toon(&engine);
    assert!(
        text.contains(&format!("Commit: {} (clean) | at_head: yes\n", &sha[..7])),
        "{text}"
    );
    assert!(!text.contains("HEAD now"), "{text}");

    let info = engine.index_info();
    assert_eq!(info.commit.as_deref(), Some(sha.as_str()));
    assert_eq!(info.dirty, Some(false));
    assert_eq!(info.state, IndexState::Ok);
}

#[test]
fn an_edit_after_the_build_is_dirty_now_and_not_at_head() {
    let (dir, sha) = git_repo();
    let repo = dir.path().join("repo");
    let engine = engine_over(dir.path(), &repo);
    std::fs::write(repo.join("src/new_file.rs"), "pub fn added() {}\n").unwrap();

    let status = engine.status_result(3, vec![]);
    assert_eq!(status.dirty, Some(false), "the index records build time");
    assert_eq!(status.dirty_now, Some(true));
    assert_eq!(status.head_now.as_deref(), Some(sha.as_str()));
    assert_eq!(status.at_head, Some(false));
    let text = toon(&engine);
    assert!(
        text.contains(&format!(
            "Commit: {0} (clean) | HEAD now: {0} (dirty) | at_head: no\n",
            &sha[..7]
        )),
        "{text}"
    );
}

#[test]
fn a_new_commit_without_a_rebuild_is_not_at_head() {
    let (dir, sha) = git_repo();
    let repo = dir.path().join("repo");
    let engine = engine_over(dir.path(), &repo);
    std::fs::write(repo.join("NOTES.txt"), "later\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "later"]);
    let newer = git(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(newer, sha);

    let status = engine.status_result(3, vec![]);
    assert_eq!(status.commit.as_deref(), Some(sha.as_str()));
    assert_eq!(status.head_now.as_deref(), Some(newer.as_str()));
    assert_eq!(status.dirty_now, Some(false));
    assert_eq!(status.at_head, Some(false));
    let text = toon(&engine);
    assert!(
        text.contains(&format!(
            "Commit: {} (clean) | HEAD now: {} (clean) | at_head: no\n",
            &sha[..7],
            &newer[..7]
        )),
        "{text}"
    );
}

#[test]
fn a_dirty_build_is_never_at_head() {
    let (dir, _) = git_repo();
    let repo = dir.path().join("repo");
    std::fs::write(repo.join("NOTES.txt"), "uncommitted\n").unwrap();
    let engine = engine_over(dir.path(), &repo);
    let status = engine.status_result(3, vec![]);
    assert_eq!(status.dirty, Some(true));
    assert_eq!(status.at_head, Some(false));
    assert!(toon(&engine).contains("(dirty)"));
}

#[test]
fn outside_a_repository_every_git_field_is_null() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(DOCS, &root);
    let engine = engine_over(dir.path(), &root);
    let status = engine.status_result(3, vec![]);
    assert_eq!(
        (
            &status.commit,
            status.dirty,
            &status.branch,
            &status.head_now,
            status.dirty_now,
            status.at_head,
            &status.git_error
        ),
        (&None, None, &None, &None, None, None, &None)
    );
    let v = serde_json::to_value(&status).unwrap();
    assert!(v["commit"].is_null() && v["at_head"].is_null() && v["head_now"].is_null());
    assert!(v.get("git_error").is_none());
    let text = toon(&engine);
    assert!(
        !text.contains("Commit:") && !text.contains("Git:"),
        "{text}"
    );
}

#[test]
fn max_depth_cuts_the_tree_and_says_so() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(DOCS, &root);
    let engine = engine_over(dir.path(), &root);

    let one = engine.status_result(1, vec![]);
    assert!(one.truncated);
    assert!(!one.tree.children.is_empty());
    assert!(one.tree.children.iter().all(|c| c.children.is_empty()));

    let default = engine.status_result(3, vec![]);
    assert_eq!(default.tree.name, ".");
    let deep = engine.status_result(99, vec![]);
    assert!(!deep.truncated);
    assert!(deep.tree.children.iter().any(|c| !c.children.is_empty()));
    // The counts do not depend on the depth.
    assert_eq!((one.files, one.symbols), (deep.files, deep.symbols));
    // `truncated` and `missing` belong to the envelope, not the summary.
    let v = serde_json::to_value(&one).unwrap();
    assert!(v.get("truncated").is_none() && v.get("missing").is_none());
}

#[test]
fn a_missing_index_is_a_result_with_the_instruction() {
    let status = StatusResult::missing(vec!["feature-a".into()]);
    assert!(status.missing);
    assert_eq!(status.files, 0);
    assert!(status.tree.children.is_empty());
    assert_eq!(status.active_features, ["feature-a"]);
    let step = status.next_step.as_deref().unwrap();
    assert!(step.contains("refresh") && step.contains("init"), "{step}");
    let v = serde_json::to_value(&status).unwrap();
    assert!(v["commit"].is_null());
    assert!(v["next_step"].is_string());
    // A loaded index has no next step.
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(DOCS, &root);
    let loaded = engine_over(dir.path(), &root).status_result(3, vec![]);
    assert!(!loaded.missing && loaded.next_step.is_none());
    assert!(serde_json::to_value(&loaded)
        .unwrap()
        .get("next_step")
        .is_none());
}

// --- Task 3.2: doc_outline max_depth ---

fn docs_engine() -> (TempDir, QueryEngine) {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    copy_fixture(DOCS, &root);
    let engine = engine_over(dir.path(), &root);
    (dir, engine)
}

#[test]
fn doc_outline_max_depth_keeps_the_top_levels() {
    let (_dir, engine) = docs_engine();
    let full = engine.doc_outline_result("docs/guide.md", None).unwrap();
    assert!(!full.truncated);
    assert_eq!(full.items.len(), 12);

    let one = engine.doc_outline_result("docs/guide.md", Some(1)).unwrap();
    assert!(one.truncated);
    assert!(one.items.iter().all(|i| i.depth == 1));
    let tops: Vec<_> = full
        .items
        .iter()
        .filter(|i| i.depth == 1)
        .cloned()
        .collect();
    assert_eq!(one.items, tops);

    let two = engine.doc_outline_result("docs/guide.md", Some(2)).unwrap();
    assert!(two.truncated);
    assert!(two.items.iter().all(|i| i.depth <= 2));
    assert!(two.items.len() > one.items.len() && two.items.len() < full.items.len());
    // The kept items are the full outline's, in order.
    let kept: Vec<_> = full
        .items
        .iter()
        .filter(|i| i.depth <= 2)
        .cloned()
        .collect();
    assert_eq!(two.items, kept);
}

#[test]
fn doc_outline_depth_counts_nesting_not_heading_level() {
    let (_dir, engine) = docs_engine();
    let full = engine.doc_outline_result("docs/guide.md", None).unwrap();
    // h3 directly under an h1 is depth 2.
    let skipped = full
        .items
        .iter()
        .find(|i| i.heading == "Skipped From H1")
        .unwrap();
    assert_eq!((skipped.level, skipped.depth), (3, 2));
    let two = engine.doc_outline_result("docs/guide.md", Some(2)).unwrap();
    assert!(two.items.iter().any(|i| i.heading == "Skipped From H1"));
    // h4 under h3 under h2 is depth 4; the skipped-level target (h3 under h2) is depth 3.
    let target = full
        .items
        .iter()
        .find(|i| i.heading == "Skipped Level Target")
        .unwrap();
    assert_eq!((target.level, target.depth), (3, 3));
    let three = engine.doc_outline_result("docs/guide.md", Some(3)).unwrap();
    assert!(three
        .items
        .iter()
        .any(|i| i.heading == "Skipped Level Target"));
    assert!(!three.items.iter().any(|i| i.heading == "Deep Detail"));
}

#[test]
fn doc_outline_without_max_depth_equals_the_view() {
    let (_dir, engine) = docs_engine();
    let view = engine.doc_outline("docs/guide.md").unwrap();
    let result = engine.doc_outline_result("docs/guide.md", None).unwrap();
    assert_eq!(result.items.len(), view.entries.len());
    assert!(!result.truncated);
    // A depth beyond the deepest changes nothing and is not truncated.
    let deep = engine
        .doc_outline_result("docs/guide.md", Some(99))
        .unwrap();
    assert_eq!(deep, result);
    // Depth 0 keeps nothing.
    let none = engine.doc_outline_result("docs/guide.md", Some(0)).unwrap();
    assert!(none.items.is_empty() && none.truncated);
    // The TOON view cut matches.
    let (cut, was_cut) = view.cut_to_depth(1);
    assert!(was_cut && cut.entries.iter().all(|e| e.depth == 0));
}
