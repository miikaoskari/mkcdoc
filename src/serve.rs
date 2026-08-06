use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::pipeline;

/// Polls a version counter and reloads the page when it changes — injected into every served
/// HTML page, never into the plain `build` output.
const RELOAD_SCRIPT: &str = r#"<script>
(function () {
  let last = null;
  setInterval(function () {
    fetch('/__mkcdoc/version')
      .then(function (res) { return res.text(); })
      .then(function (version) {
        if (last !== null && version !== last) location.reload();
        last = version;
      })
      .catch(function () {});
  }, 700);
})();
</script>
"#;

/// Build once, then watch source/content/config for changes (rebuilding on each) while serving
/// the output directory over HTTP with live reload.
pub fn run(config_path: &Path, port: u16) -> Result<()> {
    let cfg = pipeline::build(config_path)?;
    let output_dir = cfg.site.output_dir.clone();
    let version = Arc::new(AtomicU64::new(0));

    {
        let config_path = config_path.to_path_buf();
        let version = Arc::clone(&version);
        std::thread::spawn(move || watch_and_rebuild(&config_path, &version));
    }

    serve_http(&output_dir, port, &version)
}

/// Runs forever on a background thread: blocks for a filesystem event, briefly debounces
/// (editors tend to fire several events per save), then rebuilds and bumps `version` so polling
/// clients know to reload. Rebuild errors are logged and otherwise ignored — a typo mid-edit
/// shouldn't kill the dev server.
fn watch_and_rebuild(config_path: &Path, version: &Arc<AtomicU64>) {
    let cfg = match Config::load(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("mkcdoc serve: failed to read {}: {err:#}", config_path.display());
            return;
        }
    };

    let (tx, rx) = channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("mkcdoc serve: failed to start file watcher: {err:#}");
            return;
        }
    };
    let watcher: &mut RecommendedWatcher = &mut watcher;

    for dir in &cfg.source.dirs {
        watch_path(watcher, dir, RecursiveMode::Recursive);
    }
    watch_path(watcher, &cfg.content.dir, RecursiveMode::Recursive);
    watch_path(watcher, config_path, RecursiveMode::NonRecursive);

    loop {
        let Ok(event) = rx.recv() else { break };
        if !is_content_change(&event) {
            continue;
        }
        // Drain any further events for a short quiet period instead of rebuilding once per event
        // (editors typically fire several events per save).
        while rx.recv_timeout(Duration::from_millis(200)).is_ok() {}

        match pipeline::build(config_path) {
            Ok(_) => {
                version.fetch_add(1, Ordering::SeqCst);
            }
            Err(err) => eprintln!("mkcdoc serve: rebuild failed: {err:#}"),
        }
    }
}

/// Whether a filesystem event actually changed something worth rebuilding for. Excludes pure
/// reads (`Access`) and metadata-only touches (e.g. an atime bump) — without this filter, our
/// own rebuild reading the very files it's watching can generate events that trigger another
/// rebuild, which reads them again, forever.
fn is_content_change(event: &notify::Result<notify::Event>) -> bool {
    use notify::event::ModifyKind;
    use notify::EventKind;

    let Ok(event) = event else { return false };
    matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_))
    )
}

fn watch_path(watcher: &mut RecommendedWatcher, path: &Path, mode: RecursiveMode) {
    if let Err(err) = watcher.watch(path, mode) {
        eprintln!("mkcdoc serve: not watching {} ({err})", path.display());
    }
}

fn serve_http(output_dir: &Path, port: u16, version: &Arc<AtomicU64>) -> Result<()> {
    let server = tiny_http::Server::http(("127.0.0.1", port))
        .map_err(|err| anyhow::anyhow!("failed to bind 127.0.0.1:{port}: {err}"))?;
    println!("Serving {} at http://127.0.0.1:{port}", output_dir.display());

    for request in server.incoming_requests() {
        handle_request(request, output_dir, version);
    }
    Ok(())
}

fn handle_request(request: tiny_http::Request, output_dir: &Path, version: &Arc<AtomicU64>) {
    let url_path = request.url().split('?').next().unwrap_or("/").to_string();

    if url_path == "/__mkcdoc/version" {
        let body = version.load(Ordering::SeqCst).to_string();
        let _ = request.respond(tiny_http::Response::from_string(body));
        return;
    }

    let response = match resolve_path(output_dir, &url_path).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((path, bytes)) => {
            let content_type = content_type_for(&path);
            let body = if content_type.starts_with("text/html") {
                inject_reload_script(&bytes)
            } else {
                bytes
            };
            let header = tiny_http::Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes())
                .expect("static header name/value is always valid");
            tiny_http::Response::from_data(body).with_header(header)
        }
        None => tiny_http::Response::from_string("404 Not Found").with_status_code(404),
    };

    let _ = request.respond(response);
}

/// Resolve a URL path to a file inside `output_dir`, rejecting anything that would escape it
/// (e.g. `/../mkcdoc.toml`) via `..` traversal.
fn resolve_path(output_dir: &Path, url_path: &str) -> Option<PathBuf> {
    let trimmed = url_path.trim_start_matches('/');
    let candidate = if trimmed.is_empty() { "index.html" } else { trimmed };

    let root = output_dir.canonicalize().ok()?;
    let resolved = output_dir.join(candidate).canonicalize().ok()?;
    resolved.starts_with(&root).then_some(resolved)
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") | Some("mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Insert the reload-polling script just before `</body>`. Falls back to serving the bytes
/// unmodified if the file isn't valid UTF-8 or has no `</body>` tag.
fn inject_reload_script(html: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(html) else {
        return html.to_vec();
    };
    match text.rfind("</body>") {
        Some(idx) => {
            let mut out = String::with_capacity(text.len() + RELOAD_SCRIPT.len());
            out.push_str(&text[..idx]);
            out.push_str(RELOAD_SCRIPT);
            out.push_str(&text[idx..]);
            out.into_bytes()
        }
        None => html.to_vec(),
    }
}
