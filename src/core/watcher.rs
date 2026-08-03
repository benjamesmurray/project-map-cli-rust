use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::sleep;
use ignore::WalkBuilder;
use crate::core::orchestrator::Orchestrator;
use crate::core::query_engine::QueryEngine;
use crate::error::Result;

pub struct ProjectWatcher {
    root: PathBuf,
    out_dir: PathBuf,
    engine: Arc<RwLock<Option<QueryEngine>>>,
}

impl ProjectWatcher {
    pub fn new(root: PathBuf, out_dir: PathBuf, engine: Arc<RwLock<Option<QueryEngine>>>) -> Self {
        Self { root, out_dir, engine }
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
        ).map_err(|e| crate::error::AppError::Generic(format!("Watcher creation error: {}", e)))?;

        // 1. Register watches for non-ignored readable directories using ignore::WalkBuilder
        let walker = WalkBuilder::new(&self.root)
            .filter_entry(|e| {
                let name = e.file_name();
                name != ".project-map" && name != ".git"
            })
            .build();

        let mut registered_count = 0;
        for entry in walker {
            match entry {
                Ok(e) => {
                    if e.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                        let dir_path = e.path();
                        if !is_ignored_path(dir_path) {
                            if let Err(err) = watcher.watch(dir_path, RecursiveMode::NonRecursive) {
                                if is_permission_denied(&err) {
                                    tracing::warn!("Skipping unreadable directory {}: {}", dir_path.display(), err);
                                } else {
                                    tracing::warn!("Failed to watch directory {}: {}", dir_path.display(), err);
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

        tracing::info!("ProjectWatcher registered {} readable directories in {}", registered_count, self.root.display());

        let root = self.root.clone();
        let out_dir = self.out_dir.clone();
        let engine = Arc::clone(&self.engine);

        tokio::spawn(async move {
            let mut watcher = watcher;
            let debounce_duration = Duration::from_millis(500);

            loop {
                let rx_clone = rx.clone();
                let has_event = tokio::task::spawn_blocking(move || {
                    rx_clone.recv_timeout(Duration::from_millis(200))
                }).await;

                let mut received_event = false;
                if let Ok(Ok(event)) = has_event {
                    // Automatically register newly created directories
                    if matches!(event.kind, EventKind::Create(_)) {
                        for path in &event.paths {
                            if path.is_dir() && !is_ignored_path(path) {
                                if let Err(err) = watcher.watch(path, RecursiveMode::NonRecursive) {
                                    if is_permission_denied(&err) {
                                        tracing::warn!("Skipping unreadable new directory {}: {}", path.display(), err);
                                    }
                                }
                            }
                        }
                    }

                    if is_relevant_event(&event) {
                        received_event = true;
                    }
                }

                if received_event {
                    let drain_start = std::time::Instant::now();
                    while drain_start.elapsed() < debounce_duration {
                        sleep(Duration::from_millis(50)).await;
                        while rx.try_recv().is_ok() {}
                    }

                    tracing::info!("File change detected. Re-indexing project in background...");
                    let root_clone = root.clone();
                    let out_dir_clone = out_dir.clone();

                    let reindex_res = tokio::task::spawn_blocking(move || {
                        let mut orch = Orchestrator::new();
                        let _ = orch.scaffold_if_empty(&root_clone);
                        if orch.build_index(&root_clone).is_ok() && orch.save_index_versioned(&out_dir_clone).is_ok() {
                            let latest_file = out_dir_clone.join("latest").join(".project-map.json");
                            QueryEngine::load(&latest_file).ok()
                        } else {
                            None
                        }
                    }).await;

                    if let Ok(Some(new_engine)) = reindex_res {
                        if let Ok(mut lock) = engine.write() {
                            *lock = Some(new_engine);
                            tracing::info!("Index refreshed successfully in background.");
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
            io_err.kind() == std::io::ErrorKind::PermissionDenied || io_err.raw_os_error() == Some(13)
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


pub fn is_relevant_event(event: &Event) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            if event.paths.is_empty() {
                return false;
            }
            for path in &event.paths {
                if is_ignored_path(path) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

pub fn is_ignored_path(path: &Path) -> bool {
    for component in path.components() {
        let os_str = component.as_os_str();
        if os_str == ".project-map" || os_str == ".git" || os_str == "target" || os_str == "node_modules" {
            return true;
        }
    }
    false
}
