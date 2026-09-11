//! Optional development reload support. This module is absent in release builds,
//! including release builds compiled with `--features dev`.
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, sync::mpsc, thread, time::Duration};
use tower_livereload::LiveReloadLayer;

/// Keep this guard alive for as long as the server should watch its assets.
pub struct DevReload {
    layer: LiveReloadLayer,
    watcher: Option<RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
}

impl DevReload {
    pub fn layer(&self) -> LiveReloadLayer {
        self.layer.clone()
    }
}

/// Watch only the supplied paths, recursively. Atomic editor saves are coalesced
/// for 100 ms. Changes trigger a full browser reload, not state-preserving HMR.
/// Static files must also be served from disk for a reload to show new bytes.
pub fn watch(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> notify::Result<DevReload> {
    let layer = LiveReloadLayer::new();
    let reloader = layer.reloader();
    let (sender, receiver) = mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event) if !event.kind.is_access() => {
                let _ = sender.send(());
            }
            Ok(_) => {}
            Err(error) => eprintln!("[placebo:watch-error] {error}"),
        })?;
    for path in paths {
        watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;
    }
    let worker = thread::spawn(move || {
        while receiver.recv().is_ok() {
            loop {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        reloader.reload();
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    });
    Ok(DevReload {
        layer,
        watcher: Some(watcher),
        worker: Some(worker),
    })
}

impl Drop for DevReload {
    fn drop(&mut self) {
        self.watcher.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
