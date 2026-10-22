//! Task 4.5: `mcp` as a quiet, non-interactive server, tested through the spawned binary.

mod common;

use common::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    stdin: ChildStdin,
    frames: Receiver<String>,
    stderr: Arc<Mutex<Vec<u8>>>,
    started: Instant,
    next_id: u64,
}

impl Server {
    fn spawn(dir: &Path, extra: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
            .arg("mcp")
            .args(extra)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, frames) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let mut err = child.stderr.take().unwrap();
        let sink = Arc::clone(&stderr);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        Server {
            child,
            stdin,
            frames,
            stderr,
            started,
            next_id: 0,
        }
    }

    fn send(&mut self, message: Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// The next stdout line, which must be a JSON-RPC frame.
    fn recv(&mut self) -> Value {
        let line = self
            .frames
            .recv_timeout(Duration::from_secs(10))
            .expect("the server answered");
        let frame: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout carries only JSON-RPC frames, got {line:?}: {e}"));
        assert_eq!(frame["jsonrpc"], "2.0", "{line}");
        frame
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.recv();
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    fn initialize(&mut self) -> Value {
        let reply = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "server-mode-test", "version": "0"}
            }),
        );
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        reply
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
    }

    fn stderr(&self) -> Vec<u8> {
        self.stderr.lock().unwrap().clone()
    }

    fn finish(mut self) -> Vec<u8> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::thread::sleep(Duration::from_millis(100));
        self.stderr()
    }
}

fn text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

fn assert_no_ansi(stderr: &[u8]) {
    assert!(
        !stderr.windows(2).any(|w| w == b"\x1b["),
        "stderr has ANSI escapes: {:?}",
        String::from_utf8_lossy(stderr)
    );
}

#[test]
fn an_empty_directory_answers_initialize_and_status_within_two_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Server::spawn(dir.path(), &[]);

    let init = server.initialize();
    let instructions = init["result"]["instructions"]
        .as_str()
        .expect("instructions");
    assert!(instructions.contains("instead of grep"), "{instructions}");

    let status = server.call("status", json!({}));
    assert!(
        server.started.elapsed() < Duration::from_secs(2),
        "initialize and status took {:?}",
        server.started.elapsed()
    );
    assert_ne!(status["result"]["isError"], json!(true));
    assert!(
        text(&status).contains("No index found"),
        "{}",
        text(&status)
    );

    let status = server.call("status", json!({"format": "json"}));
    let envelope: Value = serde_json::from_str(&text(&status)).unwrap();
    assert_eq!(envelope["index"]["state"], "missing");
    assert_eq!(status["result"]["structuredContent"], envelope);
    assert!(envelope["summary"]["next_step"]
        .as_str()
        .unwrap()
        .contains("refresh"));

    assert_no_ansi(&server.finish());
}

#[test]
fn other_tools_return_index_missing_with_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = Server::spawn(dir.path(), &[]);
    server.initialize();

    let reply = server.call("search", json!({"query": "x"}));
    assert_eq!(reply["result"]["isError"], json!(true));
    assert_eq!(text(&reply), "Error: Index not loaded");

    let reply = server.call(
        "blast_radius",
        json!({"path": "a.rs", "symbol": "x", "format": "json"}),
    );
    assert_eq!(reply["result"]["isError"], json!(true));
    let envelope: Value = serde_json::from_str(&text(&reply)).unwrap();
    assert_eq!(envelope["error"]["code"], "index_missing");
    assert_eq!(reply["result"]["structuredContent"], envelope);

    assert_no_ansi(&server.finish());
}

/// A project directory with an index built by the binary.
fn built_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_fixture("tests/fixtures/docs", dir.path());
    let out = Command::new(env!("CARGO_BIN_EXE_project-map-cli-rust"))
        .args(["build"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    dir
}

#[cfg(target_os = "linux")]
fn inotify_fds(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|fds| {
            fds.filter_map(|e| e.ok())
                .filter_map(|e| std::fs::read_link(e.path()).ok())
                .filter(|target| target.to_string_lossy().contains("inotify"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn without_w_no_watcher_runs_and_an_edit_does_not_rebuild() {
    let dir = built_project();
    let index = dir.path().join(".project-map/latest/.project-map.json");
    let mtime = || std::fs::metadata(&index).unwrap().modified().unwrap();
    let before = mtime();
    let target_before = std::fs::read_link(dir.path().join(".project-map/latest")).ok();

    let mut server = Server::spawn(dir.path(), &[]);
    server.initialize();
    // The index on disk is served.
    let reply = server.call("search", json!({"query": "audit"}));
    assert_ne!(reply["result"]["isError"], json!(true));

    #[cfg(target_os = "linux")]
    assert_eq!(
        inotify_fds(server.child.id()),
        0,
        "no file watcher without -w"
    );

    std::fs::write(
        dir.path().join("src/audit/log.rs"),
        "pub fn brand_new_function() {}\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));

    assert_eq!(mtime(), before, "the index file was rewritten");
    assert_eq!(
        std::fs::read_link(dir.path().join(".project-map/latest")).ok(),
        target_before,
        "a new index version was published"
    );
    let reply = server.call("search", json!({"query": "brand_new_function"}));
    assert!(
        text(&reply).contains("Matches Found: 0"),
        "{}",
        text(&reply)
    );

    assert_no_ansi(&server.finish());
}

#[cfg(target_os = "linux")]
#[test]
fn with_w_a_watcher_runs_and_stderr_still_has_no_ansi() {
    let dir = built_project();
    let mut server = Server::spawn(dir.path(), &["-w"]);
    server.initialize();
    // The control for the test above: the check does see a watcher when there is one.
    let deadline = Instant::now() + Duration::from_secs(5);
    while inotify_fds(server.child.id()) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(inotify_fds(server.child.id()) > 0, "-w starts a watcher");

    // An edit makes the watcher rebuild and log through tracing; piped stderr must
    // carry plain text.
    std::fs::write(dir.path().join("src/audit/log.rs"), "pub fn changed() {}\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.stderr().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let stderr = server.finish();
    assert!(
        !stderr.is_empty(),
        "the watcher logged nothing, so the ANSI check proves nothing"
    );
    assert_no_ansi(&stderr);
}
