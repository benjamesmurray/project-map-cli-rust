//! Task 2.2: read_git_state against real repositories and fake `git` binaries.
//! Tests that change PATH hold GIT_LOCK; every test takes it so none sees a changed PATH.

mod common;

use common::{git, git_fixture, IMPORTS_FIXTURE};
use project_map_cli_rust::core::git::{read_git_state, GitBudget, GitState};
use project_map_cli_rust::core::orchestrator::Orchestrator;
use project_map_cli_rust::core::query_engine::QueryEngine;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tempfile::tempdir;

static GIT_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    GIT_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` with PATH set to `path`, restoring the old value afterwards.
fn with_path<T>(path: &str, f: impl FnOnce() -> T) -> T {
    let old = std::env::var_os("PATH");
    // SAFETY: every test in this file holds GIT_LOCK, so no other thread reads PATH meanwhile.
    unsafe { std::env::set_var("PATH", path) };
    let out = f();
    match old {
        Some(v) => unsafe { std::env::set_var("PATH", v) },
        None => unsafe { std::env::remove_var("PATH") },
    }
    out
}

fn fake_git(dir: &Path, script: &str) {
    let path = dir.join("git");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn path_with(dir: &Path) -> String {
    format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn state(root: &Path) -> GitState {
    read_git_state(root, GitBudget::BUILD)
}

#[test]
fn clean_repository_gives_commit_branch_and_not_dirty() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let sha = git_fixture(IMPORTS_FIXTURE, &temp.path().join("repo"));
    let s = state(&temp.path().join("repo"));
    assert_eq!(s.commit.as_deref(), Some(sha.as_str()));
    assert_eq!(s.commit.as_ref().unwrap().len(), 40);
    assert_eq!(s.dirty, Some(false));
    assert_eq!(s.branch.as_deref(), Some("main"));
    assert_eq!(s.error, None);
}

#[test]
fn edited_and_untracked_files_make_it_dirty_but_ignored_files_do_not() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_fixture(IMPORTS_FIXTURE, &repo);
    fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "ignore"]);
    assert_eq!(state(&repo).dirty, Some(false));

    fs::write(repo.join("ignored.txt"), "x").unwrap();
    assert_eq!(state(&repo).dirty, Some(false), "ignored file");

    fs::write(repo.join("untracked.txt"), "x").unwrap();
    assert_eq!(state(&repo).dirty, Some(true), "untracked file");
    fs::remove_file(repo.join("untracked.txt")).unwrap();
    assert_eq!(state(&repo).dirty, Some(false));

    let tracked = fs::read_dir(&repo)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_file() && p.file_name().unwrap() != ".gitignore")
        .expect("a tracked file");
    fs::write(&tracked, "edited\n").unwrap();
    assert_eq!(state(&repo).dirty, Some(true), "edited file");
}

#[test]
fn project_map_directory_does_not_make_it_dirty() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_fixture(IMPORTS_FIXTURE, &repo);
    fs::create_dir_all(repo.join(".project-map/latest")).unwrap();
    fs::write(repo.join(".project-map/latest/.project-map.json"), "{}").unwrap();
    assert_eq!(state(&repo).dirty, Some(false));
}

#[test]
fn detached_head_has_no_branch() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let sha = git_fixture(IMPORTS_FIXTURE, &repo);
    git(&repo, &["checkout", "-q", "--detach"]);
    let s = state(&repo);
    assert_eq!(s.commit.as_deref(), Some(sha.as_str()));
    assert_eq!(s.branch, None);
    assert_eq!(s.error, None);
}

#[test]
fn not_a_repository_gives_all_none() {
    let _g = lock();
    let temp = tempdir().unwrap();
    assert_eq!(state(temp.path()), GitState::default());
}

#[test]
fn repository_without_commits_has_no_commit_and_no_error() {
    let _g = lock();
    let temp = tempdir().unwrap();
    git(temp.path(), &["init", "-q", "-b", "main"]);
    let s = state(temp.path());
    assert_eq!(s.commit, None);
    assert_eq!(s.error, None);
}

#[test]
fn git_missing_gives_all_none_and_the_build_succeeds() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_fixture(IMPORTS_FIXTURE, &repo);
    let out = temp.path().join("out");
    with_path("", || {
        assert_eq!(state(&repo), GitState::default());
        let mut orch = Orchestrator::new();
        orch.build_index(&repo, &out).expect("build without git");
        let index = out.join("index.json");
        orch.save_index(&index).unwrap();
        let engine = QueryEngine::load(&index).unwrap();
        let meta = engine.meta().unwrap();
        assert_eq!(meta.git_commit, None);
        assert_eq!(meta.git_dirty, None);
        assert_eq!(meta.git_branch, None);
    });
}

#[test]
fn build_stores_git_state_in_the_index_meta() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let sha = git_fixture(IMPORTS_FIXTURE, &repo);
    let out = temp.path().join("out");
    let mut orch = Orchestrator::new();
    orch.build_index(&repo, &out).unwrap();
    let index = out.join("index.json");
    orch.save_index(&index).unwrap();
    let meta = QueryEngine::load(&index).unwrap().meta().unwrap().clone();
    assert_eq!(meta.git_commit.as_deref(), Some(sha.as_str()));
    assert_eq!(meta.git_dirty, Some(false));
    assert_eq!(meta.git_branch.as_deref(), Some("main"));
}

#[test]
fn every_git_call_sets_optional_locks_off() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_fixture(IMPORTS_FIXTURE, &repo);
    let fake = temp.path().join("bin");
    fs::create_dir_all(&fake).unwrap();
    let capture = temp.path().join("capture.txt");
    let real = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    fake_git(
        &fake,
        &format!(
            "echo \"[$GIT_OPTIONAL_LOCKS] $*\" >> {}\nexec {} \"$@\"",
            capture.display(),
            real.trim()
        ),
    );
    let s = with_path(&path_with(&fake), || state(&repo));
    assert!(s.commit.is_some());
    let log = fs::read_to_string(&capture).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 3, "rev-parse, status, symbolic-ref: {log}");
    assert!(lines.iter().all(|l| l.starts_with("[0] ")), "{log}");
}

#[test]
fn git_that_fails_reports_the_first_stderr_line() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let fake = temp.path().join("bin");
    fs::create_dir_all(&fake).unwrap();
    fake_git(
        &fake,
        "echo \"fatal: detected dubious ownership in repository at '/x'\" >&2\necho \"hint: second line\" >&2\nexit 128",
    );
    let s = with_path(&path_with(&fake), || state(temp.path()));
    assert_eq!(s.commit, None);
    assert_eq!(s.dirty, None);
    assert_eq!(s.branch, None);
    assert_eq!(
        s.error.as_deref(),
        Some("fatal: detected dubious ownership in repository at '/x'")
    );
}

#[test]
fn git_that_outlives_its_budget_gives_none_within_the_budget() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let fake = temp.path().join("bin");
    fs::create_dir_all(&fake).unwrap();
    fake_git(&fake, "exec sleep 5");
    let budget = GitBudget {
        per_call: Duration::from_millis(300),
        total: None,
    };
    let started = Instant::now();
    let s = with_path(&path_with(&fake), || read_git_state(temp.path(), budget));
    assert_eq!(s, GitState::default());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn query_budget_is_shared_across_calls() {
    let _g = lock();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    git_fixture(IMPORTS_FIXTURE, &repo);
    let fake = temp.path().join("bin");
    fs::create_dir_all(&fake).unwrap();
    let real = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    // Each call takes 0.6 s, so the 1.5 s total allows two of the three.
    fake_git(&fake, &format!("sleep 0.6\nexec {} \"$@\"", real.trim()));
    let started = Instant::now();
    let s = with_path(&path_with(&fake), || {
        read_git_state(&repo, GitBudget::QUERY)
    });
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
    assert!(s.commit.is_some());
    assert!(s.branch.is_none(), "third call is out of budget: {s:?}");
}
