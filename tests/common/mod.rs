// Shared by the test crates that use the imports fixture.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub const IMPORTS_FIXTURE: &str = "tests/fixtures/imports";

pub fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Copies a fixture tree to `dst` and returns `dst`.
pub fn copy_fixture(fixture: &str, dst: &Path) -> PathBuf {
    copy_dir(Path::new(fixture), dst);
    dst.to_path_buf()
}

/// Runs git in `dir` with a fixed identity and date, so the same tree always gives the same commit.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Fixture Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture Committer")
        .env("GIT_COMMITTER_EMAIL", "committer@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git must be installed to run the fixture tests");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Makes `dir` a Git repository with every file committed once, on branch `main`. Returns the commit SHA.
pub fn git_init_commit(dir: &Path) -> String {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "fixture"]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Copies a fixture to `dst`, makes it a Git repository with one commit and returns the SHA.
pub fn git_fixture(fixture: &str, dst: &Path) -> String {
    copy_fixture(fixture, dst);
    git_init_commit(dst)
}

/// Writes a JavaScript file larger than 256 KB (generated code, not committed), with short lines
/// so only its size makes it a build artefact. Returns its path relative to `root`.
pub fn write_large_js(root: &Path) -> &'static str {
    let rel = "js/generated.js";
    let mut src = String::from("import Stripe from \"stripe\";\n");
    let mut i = 0;
    while src.len() <= 300 * 1024 {
        src.push_str(&format!("export const value{i} = {i};\n"));
        i += 1;
    }
    std::fs::write(root.join(rel), src).unwrap();
    rel
}
