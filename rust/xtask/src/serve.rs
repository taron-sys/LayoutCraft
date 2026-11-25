//! `cargo xtask serve [port] [--host <address>]`: a static file server for local development, from the repository root, so that
//! `/web/shell.html` works. It replaces `server.mjs` (Node), with these differences: it listens on 127.0.0.1 unless `--host` says
//! otherwise (the Node server listened on every interface), serves only GET and HEAD, and refuses what could leave the folder or
//! expose hidden files such as `.git` (see `target.rs`).
//!
//! Std only: one thread per connection (at most [`MAX_CONNECTIONS`] at a time), one request per connection, time limits on reading the
//! request and on writing the response.

mod http;
mod target;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Outcome;
use http::{HeadError, Method, Request};
use target::{Found, Refusal, Target};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8137;
const PORT_VARIABLE: &str = "PORT";
const USAGE: &str = "usage: cargo xtask serve [port] [--host <address>]";
/// More simultaneous connections than this are answered with 503.
const MAX_CONNECTIONS: usize = 64;
/// The request head must arrive within this time.
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
/// A single write may block this long, and a whole response may take [`RESPONSE_TIMEOUT`].
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const COPY_BUFFER_BYTES: usize = 16 * 1024;
/// After the response the connection is drained for at most this long and this much (see `close_gracefully`).
const LINGER: Duration = Duration::from_millis(250);
const LINGER_READ: Duration = Duration::from_millis(100);
const MAX_LINGER_BYTES: usize = 64 * 1024;
/// The page to open first.
const START_PAGE: &str = "/web/shell.html";

/// The command line.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    host: String,
    port: u16,
}

impl Options {
    /// `[port] [--host <address>]`; without a port the `PORT` variable (`port_variable`) and then 8137 are used.
    fn parse(args: &[&str], port_variable: Option<&str>) -> Result<Options, String> {
        let mut host = DEFAULT_HOST.to_string();
        let mut port = None;
        let mut args = args.iter();
        while let Some(&arg) = args.next() {
            if arg == "--host" {
                host = args.next().ok_or("--host needs an address")?.to_string();
            } else if let Some(value) = arg.strip_prefix("--host=") {
                host = value.to_string();
            } else if arg.starts_with('-') || port.is_some() {
                return Err(format!("unexpected argument `{arg}`\n{USAGE}"));
            } else {
                port = Some(arg);
            }
        }
        let port = match port.or(port_variable.filter(|value| !value.is_empty())) {
            Some(text) => text.parse::<u16>().map_err(|_| format!("`{text}` is not a port number (0 to 65535)\n{USAGE}"))?,
            None => DEFAULT_PORT,
        };
        Ok(Options { host, port })
    }
}

/// What every connection needs to know.
struct Context {
    /// The served folder, canonical.
    root: PathBuf,
    /// Refuse a `Host` header that is not a local one (only when the server is bound to a loopback address).
    check_host: bool,
    head_timeout: Duration,
    response_timeout: Duration,
}

pub fn run(root: &Path, args: &[&str]) -> Outcome {
    let port_variable = std::env::var(PORT_VARIABLE).ok();
    let options = Options::parse(args, port_variable.as_deref())?;
    let root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let listener = TcpListener::bind((options.host.as_str(), options.port)).map_err(|e| bind_error(&options, &e))?;
    let address = listener.local_addr().map_err(|e| format!("cannot read the address: {e}"))?;
    println!("LayoutCraft dev server: http://{address}{START_PAGE}");
    println!("serving {} (GET and HEAD only; stop with Ctrl+C)", root.display());
    if !address.ip().is_loopback() {
        println!("WARNING: listening on {address}: anyone who can reach this address can read the files of the repository");
    }
    let context = Arc::new(Context { root, check_host: address.ip().is_loopback(), head_timeout: HEAD_TIMEOUT, response_timeout: RESPONSE_TIMEOUT });
    accept_loop(&listener, &context);
    Ok(ExitCode::FAILURE) // the loop only ends when the listener fails for good
}

fn bind_error(options: &Options, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::AddrInUse {
        format!(
            "port {} is already in use. Stop whatever is using it, or pick another port: cargo xtask serve {}",
            options.port,
            options.port.saturating_add(1)
        )
    } else {
        format!("cannot listen on {}:{}: {error}", options.host, options.port)
    }
}

/// Counts the connections that are being served.
struct Slots {
    used: AtomicUsize,
    max: usize,
}

/// One counted connection; it is given back when dropped.
struct Slot(Arc<Slots>);

impl Slots {
    fn acquire(self: &Arc<Self>) -> Option<Slot> {
        if self.used.fetch_add(1, Ordering::AcqRel) >= self.max {
            self.used.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Slot(Arc::clone(self)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}

fn accept_loop(listener: &TcpListener, context: &Arc<Context>) {
    let slots = Arc::new(Slots { used: AtomicUsize::new(0), max: MAX_CONNECTIONS });
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            std::thread::sleep(Duration::from_millis(50)); // for example out of file handles: do not spin
            continue;
        };
        match slots.acquire() {
            None => reject_busy(stream),
            Some(slot) => {
                let context = Arc::clone(context);
                // when the thread cannot be started the closure is dropped, which closes the connection and gives the slot back
                let _ = std::thread::Builder::new().name("serve".into()).spawn(move || {
                    let _slot = slot;
                    handle_connection(stream, &context);
                });
            }
        }
    }
}

fn reject_busy(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = send_error(&mut stream, 503, false);
}

/// Serves one request; every failure of the connection ends it quietly.
fn handle_connection(mut stream: TcpStream, context: &Context) {
    if stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err() {
        return;
    }
    let _ = respond(&mut stream, context);
    let _ = stream.flush();
    close_gracefully(&mut stream);
}

/// Closes the sending side and reads what the client still sends for a moment. Closing a socket with unread data resets the connection,
/// and a reset can make the client lose the response (the error page of a request that was refused before it was read to the end).
fn close_gracefully(stream: &mut TcpStream) {
    if stream.shutdown(Shutdown::Write).is_err() {
        return;
    }
    let deadline = Instant::now() + LINGER;
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    let mut drained = 0;
    while drained < MAX_LINGER_BYTES && Instant::now() < deadline {
        if stream.set_read_timeout(Some(LINGER_READ)).is_err() {
            return;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(n) => drained += n,
        }
    }
}

fn respond(stream: &mut TcpStream, context: &Context) -> io::Result<()> {
    let head = match http::read_head(stream, context.head_timeout) {
        Ok(head) => head,
        Err(HeadError::Closed) => return Ok(()),
        Err(HeadError::TimedOut) => return send_error(stream, 408, false),
        Err(HeadError::TooLarge) => return send_error(stream, 431, false),
    };
    let request = match http::parse_request(&head) {
        Ok(request) => request,
        Err(405) => return send_error_with(stream, 405, &[("Allow", "GET, HEAD")], false),
        Err(status) => return send_error(stream, status, false),
    };
    let head_only = request.method == Method::Head;
    if context.check_host && request.host.as_deref().is_some_and(|host| !http::host_allowed(host)) {
        return send_error(stream, 403, head_only);
    }
    match target::parse_target(&request.target) {
        Ok(target) => serve_target(stream, context, &request, &target),
        Err(refusal) => send_error(stream, status_of(refusal), head_only),
    }
}

fn status_of(refusal: Refusal) -> u16 {
    match refusal {
        Refusal::Malformed => 400,
        Refusal::Forbidden => 403,
        Refusal::NotFound => 404,
        Refusal::Failed => 500,
    }
}

/// A file is sent; a directory is redirected to its slash form, or answered with its `index.html`.
fn serve_target(stream: &mut TcpStream, context: &Context, request: &Request, target: &Target) -> io::Result<()> {
    let head_only = request.method == Method::Head;
    let error = |stream: &mut TcpStream, refusal: Refusal| send_error(stream, status_of(refusal), head_only);
    match target::resolve(&context.root, &target.segments) {
        Err(refusal) => error(stream, refusal),
        Ok(Found::File(_)) if target.trailing_slash => error(stream, Refusal::NotFound),
        Ok(Found::File(path)) => send_file(stream, &path, head_only, context.response_timeout),
        Ok(Found::Directory) if !target.trailing_slash => http::write_head(stream, 302, &[("Location", &target::directory_url(&target.segments))], 0),
        Ok(Found::Directory) => {
            let mut index = target.segments.clone();
            index.push("index.html".to_string());
            match target::resolve(&context.root, &index) {
                Ok(Found::File(path)) => send_file(stream, &path, head_only, context.response_timeout),
                Ok(Found::Directory) => error(stream, Refusal::NotFound),
                Err(refusal) => error(stream, refusal),
            }
        }
    }
}

fn send_error(stream: &mut TcpStream, status: u16, head_only: bool) -> io::Result<()> {
    send_error_with(stream, status, &[], head_only)
}

fn send_error_with(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)], head_only: bool) -> io::Result<()> {
    let body = http::message(status);
    let mut all = vec![("Content-Type", "text/plain; charset=utf-8")];
    all.extend_from_slice(headers);
    http::write_head(stream, status, &all, body.len() as u64)?;
    if !head_only {
        stream.write_all(body.as_bytes())?;
    }
    Ok(())
}

fn send_file(stream: &mut TcpStream, path: &Path, head_only: bool, timeout: Duration) -> io::Result<()> {
    let (file, length) = match target::open(path) {
        Ok(opened) => opened,
        Err(refusal) => return send_error(stream, status_of(refusal), head_only),
    };
    http::write_head(stream, 200, &[("Content-Type", http::content_type(path))], length)?;
    if head_only {
        return Ok(());
    }
    copy_file(file, length, stream, Instant::now() + timeout)
}

/// Copies exactly the announced number of bytes (a file that grows meanwhile is cut), and gives up when the client is too slow.
fn copy_file(file: File, length: u64, stream: &mut TcpStream, deadline: Instant) -> io::Result<()> {
    let mut reader = file.take(length);
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        if Instant::now() > deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        stream.write_all(&buffer[..read])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::thread::JoinHandle;

    #[test]
    fn without_arguments_the_server_listens_on_loopback_port_8137() {
        assert_eq!(Options::parse(&[], None), Ok(Options { host: "127.0.0.1".into(), port: 8137 }));
    }

    #[test]
    fn a_port_argument_wins_over_the_port_variable_and_an_empty_variable_is_ignored() {
        assert_eq!(Options::parse(&["9000"], Some("9100")).unwrap().port, 9000);
        assert_eq!(Options::parse(&[], Some("9100")).unwrap().port, 9100);
        assert_eq!(Options::parse(&[], Some("")).unwrap().port, 8137);
    }

    #[test]
    fn the_host_is_given_with_a_space_or_an_equals_sign_in_any_position() {
        assert_eq!(Options::parse(&["--host", "0.0.0.0", "80"], None), Ok(Options { host: "0.0.0.0".into(), port: 80 }));
        assert_eq!(Options::parse(&["81", "--host=::1"], None), Ok(Options { host: "::1".into(), port: 81 }));
    }

    #[test]
    fn a_bad_port_a_second_port_a_missing_host_or_an_unknown_option_is_refused() {
        for args in [&["x"][..], &["65536"], &["-1"], &["1", "2"], &["--host"], &["--hots", "x"], &["-p", "1"]] {
            assert!(Options::parse(args, None).is_err(), "{args:?}");
        }
        assert!(Options::parse(&[], Some("abc")).is_err());
    }

    #[test]
    fn a_connection_slot_is_counted_while_held_and_refused_beyond_the_maximum() {
        let slots = Arc::new(Slots { used: AtomicUsize::new(0), max: 2 });
        let first = slots.acquire().unwrap();
        let _second = slots.acquire().unwrap();
        assert!(slots.acquire().is_none());
        assert_eq!(slots.used.load(Ordering::Acquire), 2);
        drop(first);
        assert!(slots.acquire().is_some());
    }

    /// The served folder of the tests: a shell page, a script, a wasm file, a sub folder with an index, a hidden file and a secret next to it.
    fn site() -> (TempDir, PathBuf) {
        let outer = TempDir::new();
        outer.write("secret.txt", b"secret");
        outer.write("site/web/shell.html", b"<html>shell</html>");
        outer.write("site/web/app.js", b"export {}");
        outer.write("site/web/layoutcraft.wasm", b"\0asm\x01\0\0\0");
        outer.write("site/docs/index.html", b"<html>docs</html>");
        outer.write("site/empty/x.txt", b"x");
        outer.write("site/.git/config", b"hidden");
        let root = fs::canonicalize(outer.path().join("site")).unwrap();
        (outer, root)
    }

    fn context(root: PathBuf) -> Context {
        Context { root, check_host: true, head_timeout: Duration::from_millis(500), response_timeout: Duration::from_secs(5) }
    }

    /// Sends `request` to a server that handles one connection, and returns the whole response.
    fn exchange(context: Context, request: &[u8]) -> Vec<u8> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server: JoinHandle<()> = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handle_connection(stream, &context);
            }
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let _ = client.write_all(request); // the server may answer and close before it has read everything
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response);
        server.join().unwrap();
        response
    }

    fn get(root: &Path, target: &str) -> (u16, String, Vec<u8>) {
        request(root, &format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n"))
    }

    fn request(root: &Path, text: &str) -> (u16, String, Vec<u8>) {
        let response = exchange(context(root.to_path_buf()), text.as_bytes());
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").map_or(response.len(), |p| p + 4);
        let head = String::from_utf8_lossy(&response[..split]).into_owned();
        let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        (status, head, response[split..].to_vec())
    }

    #[test]
    fn a_file_is_served_with_its_type_its_length_and_no_cache() {
        let (_outer, root) = site();
        let (status, head, body) = get(&root, "/web/shell.html?v=1");
        assert_eq!(status, 200);
        assert_eq!(body, b"<html>shell</html>");
        assert!(head.contains("Content-Type: text/html; charset=utf-8\r\n") && head.contains("Content-Length: 18\r\n"));
        assert!(head.contains("Cache-Control: no-cache\r\n"));
    }

    #[test]
    fn a_script_and_a_wasm_file_get_their_types() {
        let (_outer, root) = site();
        assert!(get(&root, "/web/app.js").1.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
        let (status, head, body) = get(&root, "/web/layoutcraft.wasm");
        assert_eq!(status, 200);
        assert!(head.contains("Content-Type: application/wasm\r\n"));
        assert_eq!(body, b"\0asm\x01\0\0\0");
    }

    #[test]
    fn a_head_request_has_the_headers_of_a_get_and_no_body() {
        let (_outer, root) = site();
        let (status, head, body) = request(&root, "HEAD /web/shell.html HTTP/1.1\r\nHost: 127.0.0.1:8137\r\n\r\n");
        assert_eq!(status, 200);
        assert!(head.contains("Content-Length: 18\r\n"));
        assert!(body.is_empty());
    }

    #[test]
    fn a_directory_without_a_slash_is_redirected_and_with_a_slash_serves_its_index() {
        let (_outer, root) = site();
        let (status, head, _) = get(&root, "/docs?x=1");
        assert_eq!(status, 302);
        assert!(head.contains("Location: /docs/\r\n"));
        let (status, _, body) = get(&root, "/docs/");
        assert_eq!((status, body.as_slice()), (200, &b"<html>docs</html>"[..]));
    }

    #[test]
    fn a_directory_without_an_index_and_a_file_with_a_slash_are_not_found() {
        let (_outer, root) = site();
        assert_eq!(get(&root, "/empty/").0, 404);
        assert_eq!(get(&root, "/web/shell.html/").0, 404);
        assert_eq!(get(&root, "/").0, 404); // the repository root has no index.html
    }

    #[test]
    fn a_missing_file_is_not_found_with_a_plain_text_page() {
        let (_outer, root) = site();
        let (status, head, body) = get(&root, "/web/missing.js");
        assert_eq!(status, 404);
        assert_eq!(body, b"Not found");
        assert!(head.contains("Content-Type: text/plain; charset=utf-8\r\n"));
    }

    #[test]
    fn a_path_that_leaves_the_folder_is_forbidden_and_never_returns_the_file_outside() {
        let (_outer, root) = site();
        for target in [
            "/../secret.txt",
            "/%2e%2e/secret.txt",
            "/%2E%2E%2Fsecret.txt",
            "/web/..%5c..%5csecret.txt",
            "/web/../../secret.txt",
            "/..%2f..%2fsecret.txt",
            "/web/%2e%2e/%2e%2e/secret.txt",
            "/C:/Windows/win.ini",
            "/%5C%5Cserver%5Cshare%5Cx",
        ] {
            let (status, _, body) = get(&root, target);
            assert_eq!(status, 403, "{target}");
            assert!(!String::from_utf8_lossy(&body).contains("secret"), "{target}");
        }
    }

    #[test]
    fn hidden_files_are_forbidden_and_a_nul_or_a_bad_escape_is_a_bad_request() {
        let (_outer, root) = site();
        assert_eq!(get(&root, "/.git/config").0, 403);
        assert_eq!(get(&root, "/web/shell.html%00.png").0, 400);
        assert_eq!(get(&root, "/web/%zz").0, 400);
        assert_eq!(get(&root, "/%FF").0, 400);
    }

    #[test]
    fn methods_other_than_get_and_head_are_refused_with_an_allow_header() {
        let (_outer, root) = site();
        for method in ["POST", "PUT", "DELETE", "OPTIONS"] {
            let (status, head, _) = request(&root, &format!("{method} /web/shell.html HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\nbody"));
            assert_eq!(status, 405, "{method}");
            assert!(head.contains("Allow: GET, HEAD\r\n"));
        }
    }

    #[test]
    fn a_foreign_host_name_is_refused_while_the_server_is_bound_to_loopback_but_not_otherwise() {
        let (_outer, root) = site();
        assert_eq!(request(&root, "GET /web/shell.html HTTP/1.1\r\nHost: evil.example:8137\r\n\r\n").0, 403);
        let mut open = context(root);
        open.check_host = false;
        let response = exchange(open, b"GET /web/shell.html HTTP/1.1\r\nHost: evil.example\r\n\r\n");
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn garbage_gets_a_bad_request_and_another_http_version_is_not_supported() {
        let (_outer, root) = site();
        assert_eq!(request(&root, "\u{1}\u{2}garbage\r\n\r\n").0, 400);
        assert_eq!(request(&root, "GET /web/shell.html\r\n\r\n").0, 400);
        assert_eq!(request(&root, "GET /web/shell.html HTTP/3.0\r\n\r\n").0, 505);
        assert_eq!(request(&root, "GET http://localhost/web/shell.html HTTP/1.1\r\n\r\n").0, 400);
    }

    #[test]
    fn an_oversized_head_is_refused_and_a_client_that_never_finishes_its_request_gets_a_timeout() {
        let (_outer, root) = site();
        let big = format!("GET /web/shell.html HTTP/1.1\r\nX-Pad: {}\r\n\r\n", "a".repeat(http::MAX_HEAD_BYTES * 2));
        assert_eq!(request(&root, &big).0, 431);
        assert_eq!(request(&root, "GET /web/shell.html HTTP/1.1\r\nHost: localhost\r\n").0, 408);
    }

    #[test]
    fn a_client_that_connects_and_hangs_up_is_dropped_without_an_answer() {
        let (_outer, root) = site();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (stream, _) = listener.accept().unwrap();
        drop(client);
        handle_connection(stream, &context(root)); // returns at once instead of waiting for the time limit
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_folder_that_points_outside_is_forbidden_over_http() {
        let (outer, root) = site();
        std::os::unix::fs::symlink(outer.path().join("secret.txt"), root.join("web/leak.txt")).unwrap();
        std::os::unix::fs::symlink(outer.path(), root.join("web/outside")).unwrap();
        assert_eq!(get(&root, "/web/leak.txt").0, 403);
        assert_eq!(get(&root, "/web/outside/secret.txt").0, 403);
    }
}
