// Shared by the test crates that use the imports fixture.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

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

/// Builds an index of `root` (absolute) under `base` and loads an engine over it.
pub fn build_engine(
    root: &Path,
    base: &Path,
) -> project_map_cli_rust::core::query_engine::QueryEngine {
    use project_map_cli_rust::core::orchestrator::Orchestrator;
    let root = root.canonicalize().unwrap();
    let mut orch = Orchestrator::new();
    orch.build_index(&root, &base.join("out")).unwrap();
    let index = base.join("index.json");
    orch.save_index(&index).unwrap();
    project_map_cli_rust::core::query_engine::QueryEngine::load(&index).unwrap()
}

/// A fixture copied to `<tempdir>/<name>` and indexed. Keep the `TempDir` alive.
pub fn engine_for_fixture(
    fixture: &str,
    name: &str,
) -> (
    tempfile::TempDir,
    project_map_cli_rust::core::query_engine::QueryEngine,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = copy_fixture(fixture, &dir.path().join(name));
    let engine = build_engine(&root, dir.path());
    (dir, engine)
}

/// A stand-in for the MCP runtime handlers receive; the tools never touch it.
pub struct DummyRuntime;
#[async_trait::async_trait]
impl rust_mcp_sdk::McpServer for DummyRuntime {
    async fn start(self: Arc<Self>) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    async fn set_client_details(
        &self,
        _: rust_mcp_sdk::schema::InitializeRequestParams,
    ) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    fn server_info(&self) -> &rust_mcp_sdk::schema::InitializeResult {
        todo!()
    }
    fn client_info(&self) -> Option<rust_mcp_sdk::schema::InitializeRequestParams> {
        todo!()
    }
    async fn auth_info<'a>(
        &'a self,
    ) -> tokio::sync::RwLockReadGuard<'a, Option<rust_mcp_sdk::auth::AuthInfo>> {
        todo!()
    }
    async fn auth_info_cloned(&self) -> Option<rust_mcp_sdk::auth::AuthInfo> {
        todo!()
    }
    async fn update_auth_info(&self, _: Option<rust_mcp_sdk::auth::AuthInfo>) {
        todo!()
    }
    async fn wait_for_initialization(&self) {
        todo!()
    }
    fn task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ServerTaskStore>> {
        todo!()
    }
    fn client_task_store(&self) -> Option<Arc<rust_mcp_sdk::task_store::ClientTaskStore>> {
        todo!()
    }
    async fn stderr_message(&self, _: String) -> rust_mcp_sdk::error::SdkResult<()> {
        todo!()
    }
    fn session_id(&self) -> Option<String> {
        todo!()
    }
    async fn send(
        &self,
        _: rust_mcp_sdk::schema::schema_utils::MessageFromServer,
        _: Option<rust_mcp_sdk::schema::RequestId>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<Option<rust_mcp_sdk::schema::schema_utils::ClientMessage>>
    {
        todo!()
    }
    async fn send_batch(
        &self,
        _: Vec<rust_mcp_sdk::schema::schema_utils::ServerMessage>,
        _: Option<std::time::Duration>,
    ) -> rust_mcp_sdk::error::SdkResult<
        Option<Vec<rust_mcp_sdk::schema::schema_utils::ClientMessage>>,
    > {
        todo!()
    }
}
