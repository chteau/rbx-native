//! `rbxview --serve`: what the browser build (`web`) talks to.
//!
//! A page cannot fetch from Roblox's asset CDNs — they send no CORS headers
//! — and has no disk cache, no Open Cloud key and no Studio install to read
//! `rbxasset://` content out of. This process has all four, through the very
//! resolver `rbxview` itself loads with (`assets::resolver`), and hands the
//! page the resolved bytes; decoding stays in the page. Alongside that it
//! serves the page itself, and optionally one place file at `/place`, which
//! the page opens by itself.
//!
//! Plain HTTP/1.1, one thread per connection, `Connection: close`: a browser
//! keeps at most six connections to one host, which is also how many
//! requests `rbxview`'s own pool keeps in flight against Open Cloud (see
//! `assets::WORKERS`). Bound to loopback only — the asset route spends the
//! machine's own API key on whatever id it is asked for.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rbx_assets::{AssetRef, AssetResolver};

use crate::assets;

pub const DEFAULT_PORT: u16 = 8123;

/// Serves `web` (the built page — see `scripts/build-web.sh`), the asset
/// route, and `place` at `/place` if given, until the process is killed.
pub fn serve(web: &Path, place: Option<&Path>, port: u16) -> Result<(), String> {
    let resolver = Arc::new(assets::resolver()?);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format!("cannot listen on 127.0.0.1:{port}: {err}"))?;
    let root = Arc::new(web.to_path_buf());
    let place = Arc::new(place.map(Path::to_path_buf));
    eprintln!(
        "rbxview: serving {} on http://127.0.0.1:{port}/",
        web.display()
    );

    for stream in listener.incoming().flatten() {
        let (resolver, root, place) = (resolver.clone(), root.clone(), place.clone());
        std::thread::spawn(move || {
            if let Err(err) = handle(stream, &resolver, &root, place.as_ref().as_deref()) {
                eprintln!("rbxview: {err}");
            }
        });
    }
    Ok(())
}

fn handle(
    stream: TcpStream,
    resolver: &AssetResolver,
    root: &Path,
    place: Option<&Path>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    // The rest of the head says nothing this server acts on, but has to be
    // read off the socket before the answer goes out.
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }
    let target = match line.split_whitespace().collect::<Vec<_>>()[..] {
        ["GET", target, _] => target.split('?').next().unwrap_or(""),
        _ => return respond(reader.get_mut(), 405, "text/plain", b"GET only"),
    };
    let (status, kind, body) = route(target, resolver, root, place);
    respond(reader.get_mut(), status, kind, &body)
}

fn route(
    target: &str,
    resolver: &AssetResolver,
    root: &Path,
    place: Option<&Path>,
) -> (u16, &'static str, Vec<u8>) {
    if let Some(reference) = target.strip_prefix("/asset/") {
        return asset(reference, resolver);
    }
    if target == "/place" {
        return match place.map(std::fs::read) {
            Some(Ok(bytes)) => (200, "application/octet-stream", bytes),
            _ => (404, "text/plain", b"no place".to_vec()),
        };
    }
    match file(root, &decode(target)) {
        Some((kind, bytes)) => (200, kind, bytes),
        None => (404, "text/plain", b"not found".to_vec()),
    }
}

/// `/asset/id/<n>` or `/asset/native/<path>` — the two forms the page's
/// fetcher asks with (see `load::fetcher::web::path`). The status is the
/// resolver's verdict: 404 for a failure asking again cannot change, 503 for
/// one of the moment, which the page retries the way `load::Resident` does.
fn asset(reference: &str, resolver: &AssetResolver) -> (u16, &'static str, Vec<u8>) {
    let reference = match reference.split_once('/') {
        Some(("id", id)) => id.parse().ok().map(AssetRef::Id),
        Some(("native", path)) => Some(AssetRef::Native(decode(path))),
        _ => None,
    };
    let Some(reference) = reference else {
        return (404, "text/plain", b"not an asset reference".to_vec());
    };
    match resolver.resolve(&reference) {
        Ok(asset) => (200, "application/octet-stream", asset.bytes),
        Err(err) => {
            let failure = assets::Failure::resolving(&reference, &err);
            let status = if failure.transient { 503 } else { 404 };
            (status, "text/plain", failure.warning.into_bytes())
        }
    }
}

/// A file under `root`, `/` being `index.html`. Anything that climbs out of
/// `root` is not found.
fn file(root: &Path, target: &str) -> Option<(&'static str, Vec<u8>)> {
    let relative = PathBuf::from(target.trim_start_matches('/'));
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let mut path = root.join(relative);
    if path.is_dir() {
        path.push("index.html");
    }
    let kind = match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        // Required as is for `WebAssembly.instantiateStreaming`.
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        _ => "application/octet-stream",
    };
    Some((kind, std::fs::read(path).ok()?))
}

fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Service Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

/// Percent-decoding, for a path the page ran through `encodeURI`. A
/// malformed escape is kept as written.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{decode, file};

    #[test]
    fn decodes_what_encode_uri_writes() {
        assert_eq!(decode("fonts/Source%20Sans.json"), "fonts/Source Sans.json");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn nothing_outside_the_root_is_served() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("index.html"), "hi").unwrap();
        assert_eq!(file(root.path(), "/").unwrap().1, b"hi");
        assert!(file(root.path(), "/../Cargo.toml").is_none());
        assert!(file(root.path(), "//etc/passwd").is_none());
    }
}
