//! Git state of the indexed tree, read through the `git` binary.
//!
//! Every failure gives `None` for the affected field and never fails the caller.
//! A Git that runs but fails (e.g. "dubious ownership") also reports its first
//! stderr line in `GitState::error`.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long Git may take. `per_call` bounds each invocation, `total` bounds all of them together.
#[derive(Debug, Clone, Copy)]
pub struct GitBudget {
    pub per_call: Duration,
    pub total: Option<Duration>,
}

impl GitBudget {
    /// Build time: 5 seconds per call.
    pub const BUILD: GitBudget = GitBudget {
        per_call: Duration::from_secs(5),
        total: None,
    };
    /// Query time: one 1.5-second budget for all calls.
    pub const QUERY: GitBudget = GitBudget {
        per_call: Duration::from_millis(1500),
        total: Some(Duration::from_millis(1500)),
    };
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitState {
    /// Full 40-hex SHA of HEAD.
    pub commit: Option<String>,
    /// Tracked changes or untracked files; ignored files do not count.
    pub dirty: Option<bool>,
    /// None when HEAD is detached.
    pub branch: Option<String>,
    /// First stderr line when Git ran but failed.
    pub error: Option<String>,
}

enum Outcome {
    /// Exit status 0 with stdout.
    Ok(String),
    /// Non-zero exit with the first stderr line (possibly empty).
    Failed(String),
    /// Git missing, timed out or unreadable.
    Unavailable,
}

pub fn read_git_state(root: &Path, budget: GitBudget) -> GitState {
    let started = Instant::now();
    let mut state = GitState::default();
    let remaining = |started: Instant| -> Duration {
        match budget.total {
            Some(total) => total.saturating_sub(started.elapsed()).min(budget.per_call),
            None => budget.per_call,
        }
    };

    match run_git(root, &["rev-parse", "HEAD"], remaining(started)) {
        Outcome::Ok(out) => {
            let sha = out.trim();
            if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                state.commit = Some(sha.to_string());
            }
        }
        Outcome::Failed(line) => {
            if is_not_a_repository(&line) {
                return state;
            }
            // A repository without commits has no HEAD; that is not a Git failure.
            if !is_unborn_head(&line) {
                return fail(state, line);
            }
        }
        Outcome::Unavailable => return state,
    }

    let dirty = run_git(
        root,
        &["status", "--porcelain", "--", ".", ":(exclude).project-map"],
        remaining(started),
    );
    match dirty {
        Outcome::Ok(out) => state.dirty = Some(!out.trim().is_empty()),
        Outcome::Failed(line) if !line.is_empty() => return fail(state, line),
        _ => {}
    }

    // Exits 1 with no message when HEAD is detached.
    match run_git(
        root,
        &["symbolic-ref", "--short", "-q", "HEAD"],
        remaining(started),
    ) {
        Outcome::Ok(out) => {
            let name = out.trim();
            if !name.is_empty() {
                state.branch = Some(name.to_string());
            }
        }
        Outcome::Failed(line) if !line.is_empty() => return fail(state, line),
        _ => {}
    }
    state
}

fn fail(mut state: GitState, line: String) -> GitState {
    eprintln!("warning: git failed: {line}");
    state.error = Some(line);
    state
}

fn is_not_a_repository(line: &str) -> bool {
    line.contains("not a git repository")
}

fn is_unborn_head(line: &str) -> bool {
    line.contains("unknown revision")
        || line.contains("ambiguous argument 'HEAD'")
        || line.contains("bad revision 'HEAD'")
}

fn run_git(root: &Path, args: &[&str], timeout: Duration) -> Outcome {
    let mut child = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Outcome::Unavailable,
    };

    // Drain both pipes on threads so a large `status` cannot block the child.
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break None,
        }
    };

    let status = match status {
        Some(s) => s,
        // Do not join: a grandchild may still hold the pipes open.
        None => return Outcome::Unavailable,
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    if status.success() {
        Outcome::Ok(stdout)
    } else {
        Outcome::Failed(stderr.lines().next().unwrap_or("").trim().to_string())
    }
}
