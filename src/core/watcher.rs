use crate::core::query_engine::QueryEngine;
use crate::error::Result;
use ignore::WalkBuilder;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::sleep;

pub struct ProjectWatcher {
    root: PathBuf,
    out_dir: PathBuf,
    engine: Arc<RwLock<Option<QueryEngine>>>,
    lock_timeout: Duration,
}

impl ProjectWatcher {
    pub fn new(root: PathBuf, out_dir: PathBuf, engine: Arc<RwLock<Option<QueryEngine>>>) -> Self {
        Self {
            root,
            out_dir,
            engine,
            lock_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    pub fn start_in_background(self) -> Result<()> {
        let (tx, rx) = crossbeam_channel::unbounded();

        let mut watcher = notify::RecommendedWatcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            notify::Config::default(),
        )
        .map_err(|e| crate::error::AppError::Generic(format!("Watcher creation error: {}", e)))?;

        // 1. Register watches for non-ignored readable directories using ignore::WalkBuilder
        let walker = WalkBuilder::new(&self.root)
            .filter_entry({
                let out_dir = self.out_dir.clone();
                let can_out = self.out_dir.canonicalize().ok();
                move |e| {
                    let name = e.file_name();
                    if name == ".project-map"
                        || name == ".git"
                        || name == "target"
                        || name == "node_modules"
                    {
                        return false;
                    }
                    let p = e.path();
                    if p == out_dir || p.starts_with(&out_dir) {
                        return false;
                    }
                    if let (Some(ref can_out), Ok(can_p)) = (&can_out, p.canonicalize()) {
                        if can_p == *can_out || can_p.starts_with(can_out) {
                            return false;
                        }
                    }
                    true
                }
            })
            .build();

        let mut registered_count = 0;
        for entry in walker {
            match entry {
                Ok(e) => {
                    if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        let dir_path = e.path();
                        if !is_ignored_path(dir_path, Some(&self.out_dir)) {
                            if let Err(err) = watcher.watch(dir_path, RecursiveMode::NonRecursive) {
                                if is_permission_denied(&err) {
                                    tracing::warn!(
                                        "Skipping unreadable directory {}: {}",
                                        dir_path.display(),
                                        err
                                    );
                                } else {
                                    tracing::warn!(
                                        "Failed to watch directory {}: {}",
                                        dir_path.display(),
                                        err
                                    );
                                }
                            } else {
                                registered_count += 1;
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!("Skipping unreadable path during directory walk: {}", err);
                }
            }
        }

        tracing::info!(
            "ProjectWatcher registered {} readable directories in {}",
            registered_count,
            self.root.display()
        );

        let root = self.root.clone();
        let out_dir = self.out_dir.clone();
        let engine = Arc::clone(&self.engine);
        let lock_timeout = self.lock_timeout;

        tokio::spawn(async move {
            let mut watcher = watcher;
            let debounce_duration = Duration::from_millis(500);

            loop {
                let rx_clone = rx.clone();
                let has_event = tokio::task::spawn_blocking(move || {
                    rx_clone.recv_timeout(Duration::from_millis(200))
                })
                .await;

                let mut received_event = false;
                let mut first_event_time = std::time::SystemTime::now();

                if let Ok(Ok(event)) = has_event {
                    first_event_time = std::time::SystemTime::now();
                    // Automatically register newly created directories
                    if matches!(event.kind, EventKind::Create(_)) {
                        for path in &event.paths {
                            if path.is_dir() && !is_ignored_path(path, Some(&out_dir)) {
                                if let Err(err) = watcher.watch(path, RecursiveMode::NonRecursive) {
                                    if is_permission_denied(&err) {
                                        tracing::warn!(
                                            "Skipping unreadable new directory {}: {}",
                                            path.display(),
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if is_relevant_event(&event, Some(&out_dir)) {
                        received_event = true;
                    }
                }

                if received_event {
                    let drain_start = std::time::Instant::now();
                    while drain_start.elapsed() < debounce_duration {
                        sleep(Duration::from_millis(50)).await;
                        while rx.try_recv().is_ok() {}
                    }

                    tracing::info!(
                        "File change detected. Processing index update in background..."
                    );
                    let root_clone = root.clone();
                    let out_dir_clone = out_dir.clone();

                    let update_res = tokio::task::spawn_blocking(move || -> std::result::Result<Option<QueryEngine>, String> {
                        use crate::core::lock::IndexLock;
                        use crate::core::orchestrator::Orchestrator;
                        use std::fs;

                        // Acquire IndexLock with configured timeout
                        let _lock_guard = match IndexLock::acquire(&out_dir_clone, lock_timeout) {
                            Ok(g) => g,
                            Err(e) => {
                                tracing::warn!("Watcher timed out or failed to acquire IndexLock ({:?}); skipping cycle to retry on next event: {}", lock_timeout, e);
                                return Ok(None);
                            }
                        };

                        let latest_file = out_dir_clone.join("latest").join(".project-map.json");

                        // Check if latest index was already rebuilt newer than the triggering event
                        let should_rebuild = if latest_file.exists() {
                            let mut newer = false;
                            // Check via metadata mtime or parse meta.built_at
                            if let Ok(metadata) = fs::metadata(&latest_file) {
                                if let Ok(mtime) = metadata.modified() {
                                    if mtime > first_event_time {
                                        newer = true;
                                    }
                                }
                            }
                            if !newer {
                                if let Ok(existing) = QueryEngine::load(&latest_file) {
                                    if let Some(meta) = existing.meta() {
                                        if let Ok(built_dt) = chrono::DateTime::parse_from_rfc3339(&meta.built_at) {
                                            let built_sys_time: std::time::SystemTime = built_dt.into();
                                            if built_sys_time > first_event_time {
                                                newer = true;
                                            }
                                        }
                                    }
                                }
                            }
                            !newer
                        } else {
                            true
                        };

                        if should_rebuild {
                            if let Err(e) = crate::core::orchestrator::ensure_index_not_newer(&out_dir_clone) {
                                tracing::warn!("Watcher skipping rebuild: {}", e);
                                return Ok(None);
                            }
                            tracing::info!("Rebuilding index under IndexLock...");
                            let mut orch = Orchestrator::new();
                            orch.scaffold_if_empty(&root_clone, &out_dir_clone).map_err(|e| e.to_string())?;
                            orch.build_index(&root_clone, &out_dir_clone).map_err(|e| e.to_string())?;
                            match orch.save_index_versioned_unlocked(&out_dir_clone) {
                                Ok(()) => {}
                                Err(e @ crate::error::AppError::NewerIndex { .. }) => {
                                    tracing::warn!("Watcher skipping rebuild: {}", e);
                                    return Ok(None);
                                }
                                Err(e) => return Err(e.to_string()),
                            }
                        } else {
                            tracing::info!("Index already rebuilt by another process (coalesced). Reloading only.");
                        }

                        // Reload query engine with retry policy
                        match QueryEngine::load_with_retry(&latest_file, crate::core::query_engine::RetryPolicy::default()) {
                            Ok(loaded_engine) => Ok(Some(loaded_engine)),
                            Err(err) => {
                                tracing::warn!("Failed to reload index from {:?}: {}. Retaining previous engine.", latest_file, err);
                                Err(err.to_string())
                            }
                        }
                    }).await;

                    match update_res {
                        Ok(Ok(Some(new_engine))) => {
                            if let Ok(mut lock) = engine.write() {
                                *lock = Some(new_engine);
                                tracing::info!("Index refreshed successfully in background.");
                            }
                        }
                        Ok(Ok(None)) => {
                            // Skipped due to lock timeout or intentional skip
                        }
                        Ok(Err(err_msg)) => {
                            tracing::warn!(
                                "Index update encountered error: {}. Previous engine retained.",
                                err_msg
                            );
                        }
                        Err(join_err) => {
                            tracing::warn!(
                                "Background indexing task panicked or failed: {}",
                                join_err
                            );
                        }
                    }
                }
            }
        });

        Ok(())
    }
}

pub fn is_permission_denied(err: &notify::Error) -> bool {
    match &err.kind {
        notify::ErrorKind::Io(io_err) => {
            io_err.kind() == std::io::ErrorKind::PermissionDenied
                || io_err.raw_os_error() == Some(13)
        }
        notify::ErrorKind::Generic(msg) => {
            let lower = msg.to_lowercase();
            lower.contains("permission denied") || lower.contains("os error 13")
        }
        _ => {
            let msg = err.to_string().to_lowercase();
            msg.contains("permission denied") || msg.contains("os error 13")
        }
    }
}

pub fn is_relevant_event(event: &Event, out_dir: Option<&Path>) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            if event.paths.is_empty() {
                return false;
            }
            for path in &event.paths {
                if is_ignored_path(path, out_dir) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn is_ignored_path(path: &Path, out_dir: Option<&Path>) -> bool {
    for component in path.components() {
        let os_str = component.as_os_str();
        if os_str == ".project-map"
            || os_str == ".git"
            || os_str == "target"
            || os_str == "node_modules"
        {
            return true;
        }
    }
    if let Some(out) = out_dir {
        if path == out || path.starts_with(out) {
            return true;
        }
        if let (Ok(can_path), Ok(can_out)) = (path.canonicalize(), out.canonicalize()) {
            if can_path == can_out || can_path.starts_with(&can_out) {
                return true;
            }
        }
    }
    false
}
