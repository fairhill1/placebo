//! Optional development reload support. This module is absent in release builds,
//! including release builds compiled with `--features dev`.
use axum::{body::Body, http::Request};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, sync::mpsc, thread, time::Duration};
use tower_livereload::LiveReloadLayer;

type Layer = LiveReloadLayer<fn(&Request<Body>) -> bool>;

/// The reload script goes into pages the browser loads. A page the runtime
/// reads or a save's reply is shown inside the loaded one, and a linked page
/// runs its scripts, so a copy there would open one more reload stream per
/// click.
fn loaded(request: &Request<Body>) -> bool {
    let headers = request.headers();
    !headers.contains_key("x-placebo-refresh") && !headers.contains_key("x-placebo-request")
}

/// Keep this guard alive for as long as the server should watch its assets.
pub struct DevReload {
    layer: Layer,
    watcher: Option<RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
}

impl DevReload {
    pub fn layer(&self) -> Layer {
        self.layer.clone()
    }
}

/// Watch only the supplied paths, recursively. Atomic editor saves are coalesced
/// for 100 ms. Changes trigger a full browser reload, not state-preserving HMR.
/// Static files must also be served from disk for a reload to show new bytes.
pub fn watch(paths: impl IntoIterator<Item = impl AsRef<Path>>) -> notify::Result<DevReload> {
    let layer = LiveReloadLayer::new().request_predicate(loaded as fn(&Request<Body>) -> bool);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pages_the_browser_loads_get_the_reload_script() {
        let request = |header: Option<&str>| {
            let mut request = Request::builder().uri("/");
            if let Some(header) = header {
                request = request.header(header, "6");
            }
            request.body(Body::empty()).unwrap()
        };
        assert!(loaded(&request(None)));
        assert!(!loaded(&request(Some("x-placebo-refresh"))));
        assert!(!loaded(&request(Some("x-placebo-request"))));
    }
}
