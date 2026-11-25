//! The little HTTP/1.1 that the development server needs: read the head of a request within a time limit, parse the request line and the
//! `Host` header, and write a response head. One request per connection (`Connection: close`).

use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

/// A request head larger than this is refused (431).
pub const MAX_HEAD_BYTES: usize = 16 * 1024;
const END_OF_HEAD: &[u8] = b"\r\n\r\n";

/// The methods that are served.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Method {
    Get,
    Head,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub target: String,
    /// The value of the `Host` header, if the request has one.
    pub host: Option<String>,
}

/// Why the head of a request could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum HeadError {
    /// The client closed the connection (or sent nothing) before the head was complete: nothing to answer.
    Closed,
    /// The head did not arrive in time (408).
    TimedOut,
    /// The head is larger than [`MAX_HEAD_BYTES`] (431).
    TooLarge,
}

/// Reads up to the empty line that ends the head. The whole head must arrive within `limit`, so a client that sends a byte at a time
/// cannot hold the connection. Bytes after the head (a body) are ignored.
pub fn read_head(stream: &mut TcpStream, limit: Duration) -> Result<Vec<u8>, HeadError> {
    let deadline = Instant::now() + limit;
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return Err(HeadError::TimedOut);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(HeadError::Closed),
            Ok(n) => head.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Err(HeadError::TimedOut),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(HeadError::Closed),
        }
        if let Some(end) = find(&head, END_OF_HEAD) {
            head.truncate(end);
            return Ok(head);
        }
        if head.len() > MAX_HEAD_BYTES {
            return Err(HeadError::TooLarge);
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Parses the head (without the final empty line). The error is the status to answer with.
pub fn parse_request(head: &[u8]) -> Result<Request, u16> {
    let text = std::str::from_utf8(head).map_err(|_| 400u16)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let parts: Vec<&str> = request_line.split(' ').collect();
    let [method, target, version] = parts.as_slice() else { return Err(400) };
    if target.is_empty() || method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(400);
    }
    match *version {
        "HTTP/1.1" | "HTTP/1.0" => {}
        other if other.starts_with("HTTP/") => return Err(505),
        _ => return Err(400),
    }
    let host = host_header(lines)?;
    let method = match *method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        _ => return Err(405),
    };
    Ok(Request { method, target: (*target).to_string(), host })
}

/// The `Host` header of the header lines; a line without a colon, or two `Host` headers, is malformed.
fn host_header<'a>(lines: impl Iterator<Item = &'a str>) -> Result<Option<String>, u16> {
    let mut host = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        if name.eq_ignore_ascii_case("host") && host.replace(value.trim().to_string()).is_some() {
            return Err(400);
        }
    }
    Ok(host)
}

/// Is the `Host` header one of a local server: an IP address or `localhost` (and `*.localhost`), with or without a port? A page on
/// another site that makes the browser's name for it point at this machine ("DNS rebinding") sends its own name, and is refused.
pub fn host_allowed(value: &str) -> bool {
    let name = match value.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => value.rsplit_once(':').map_or(value, |(name, port)| if port.bytes().all(|b| b.is_ascii_digit()) { name } else { value }),
    };
    let name = name.to_ascii_lowercase();
    name.parse::<IpAddr>().is_ok() || name == "localhost" || name.ends_with(".localhost")
}

/// The reason phrase of a status.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Internal Server Error",
    }
}

/// The text of an error page.
pub fn message(status: u16) -> &'static str {
    match status {
        400 => "Bad request",
        403 => "Forbidden",
        404 => "Not found",
        405 => "Method not allowed",
        408 => "Request timeout",
        431 => "Request header too large",
        503 => "Server busy",
        505 => "HTTP version not supported",
        _ => "Server error",
    }
}

/// The `Content-Type` of a file by its extension (the table of the Node server this replaces, in any case of the extension).
pub fn content_type(path: &Path) -> &'static str {
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// Writes the status line and headers. `extra` are the headers of this response; every response is `no-cache`, closes the connection and
/// forbids content sniffing.
pub fn write_head(out: &mut impl Write, status: u16, extra: &[(&str, &str)], content_length: u64) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Cache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: {content_length}\r\n\r\n"
    ));
    out.write_all(head.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn parse(text: &str) -> Result<Request, u16> {
        parse_request(text.as_bytes())
    }

    #[test]
    fn a_get_and_a_head_request_are_parsed_with_their_host() {
        let get = parse("GET /web/shell.html?x=1 HTTP/1.1\r\nHost: localhost:8137\r\nAccept: */*").unwrap();
        assert_eq!(get, Request { method: Method::Get, target: "/web/shell.html?x=1".into(), host: Some("localhost:8137".into()) });
        assert_eq!(parse("HEAD / HTTP/1.0").unwrap().method, Method::Head);
        assert_eq!(parse("GET / HTTP/1.0").unwrap().host, None);
    }

    #[test]
    fn other_methods_are_not_allowed_and_a_lower_case_method_is_malformed() {
        for method in ["POST", "PUT", "DELETE", "OPTIONS", "TRACE", "CONNECT", "PATCH"] {
            assert_eq!(parse(&format!("{method} / HTTP/1.1")), Err(405), "{method}");
        }
        assert_eq!(parse("get / HTTP/1.1"), Err(400));
        assert_eq!(parse("GE T / HTTP/1.1"), Err(400));
    }

    #[test]
    fn a_malformed_request_line_is_a_bad_request() {
        for line in
            ["", "GET", "GET /", "GET / HTTP/1.1 extra", "GET  / HTTP/1.1", " GET / HTTP/1.1", "GET / HTTX/1.1", "GET / 1.1", "\u{0}\u{1}\u{2}"]
        {
            assert_eq!(parse(line), Err(400), "{line:?}");
        }
        assert_eq!(parse_request(b"GET / HTTP/1.1\r\nHost: \xff"), Err(400));
    }

    #[test]
    fn another_version_of_http_is_not_supported() {
        assert_eq!(parse("GET / HTTP/2.0"), Err(505));
        assert_eq!(parse("GET / HTTP/0.9"), Err(505));
    }

    #[test]
    fn a_header_line_without_a_colon_or_a_second_host_is_a_bad_request() {
        assert_eq!(parse("GET / HTTP/1.1\r\nnonsense"), Err(400));
        assert_eq!(parse("GET / HTTP/1.1\r\nHost: a\r\nhost: b"), Err(400));
        assert_eq!(parse("GET / HTTP/1.1\r\nX-Host: a\r\nHOST:b").unwrap().host.as_deref(), Some("b"));
    }

    #[test]
    fn only_ip_addresses_and_localhost_names_are_local_hosts() {
        for host in [
            "localhost",
            "localhost:8137",
            "LOCALHOST",
            "127.0.0.1",
            "127.0.0.1:8137",
            "[::1]",
            "[::1]:8137",
            "app.localhost:80",
            "192.168.1.5",
            "0.0.0.0:1",
        ] {
            assert!(host_allowed(host), "{host}");
        }
        for host in [
            "evil.example",
            "evil.example:8137",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "",
            "notlocalhost",
            "[evil",
            "x:y",
            "localhost:80:90",
        ] {
            assert!(!host_allowed(host), "{host}");
        }
    }

    #[test]
    fn the_content_type_follows_the_extension_of_the_old_server_and_defaults_to_binary() {
        let types = |file: &str| content_type(Path::new(file));
        assert_eq!(types("web/shell.html"), "text/html; charset=utf-8");
        assert_eq!(types("web/shell.js"), "text/javascript; charset=utf-8");
        assert_eq!(types("web/mcpinfo.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(types("a.css"), "text/css; charset=utf-8");
        assert_eq!(types("a.json"), "application/json; charset=utf-8");
        assert_eq!(types("a.svg"), "image/svg+xml");
        assert_eq!(types("a.png"), "image/png");
        assert_eq!(types("a.ico"), "image/x-icon");
        assert_eq!(types("web/layoutcraft.wasm"), "application/wasm");
        assert_eq!(types("web/layoutcraft.WASM"), "application/wasm");
        assert_eq!(types("a.vsdx"), "application/octet-stream");
        assert_eq!(types("noext"), "application/octet-stream");
    }

    #[test]
    fn a_response_head_has_the_status_the_extra_headers_and_the_fixed_ones() {
        let mut out = Vec::new();
        write_head(&mut out, 302, &[("Location", "/web/")], 0).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 302 Found\r\nLocation: /web/\r\n"));
        assert!(text.contains("Cache-Control: no-cache\r\n") && text.contains("Connection: close\r\n") && text.contains("Content-Length: 0\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    /// A connected pair: the server's end and the client's end.
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (listener.accept().unwrap().0, client)
    }

    #[test]
    fn the_head_is_read_across_several_packets_and_a_body_after_it_is_left_alone() {
        let (mut server, mut client) = pair();
        client.write_all(b"GET / HT").unwrap();
        client.flush().unwrap();
        let writer = std::thread::spawn(move || {
            client.write_all(b"TP/1.1\r\nHost: x\r\n\r\nBODY").unwrap();
            client
        });
        let head = read_head(&mut server, Duration::from_secs(5)).unwrap();
        assert_eq!(head, b"GET / HTTP/1.1\r\nHost: x");
        drop(writer.join().unwrap());
    }

    #[test]
    fn a_client_that_sends_nothing_or_too_slowly_times_out_and_one_that_hangs_up_is_closed() {
        let (mut server, client) = pair();
        assert_eq!(read_head(&mut server, Duration::from_millis(100)), Err(HeadError::TimedOut));
        drop(client);
        assert_eq!(read_head(&mut server, Duration::from_secs(5)), Err(HeadError::Closed));
    }

    #[test]
    fn a_head_that_never_ends_is_too_large() {
        let (mut server, mut client) = pair();
        let writer = std::thread::spawn(move || {
            let _ = client.write_all(&vec![b'a'; MAX_HEAD_BYTES * 2]);
            client
        });
        assert_eq!(read_head(&mut server, Duration::from_secs(5)), Err(HeadError::TooLarge));
        drop(server);
        drop(writer.join().unwrap());
    }

    #[test]
    fn the_time_limit_covers_the_whole_head_not_each_read() {
        let (mut server, mut client) = pair();
        let writer = std::thread::spawn(move || {
            for _ in 0..40 {
                if client.write_all(b"a").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            client
        });
        assert_eq!(read_head(&mut server, Duration::from_millis(300)), Err(HeadError::TimedOut));
        drop(server);
        drop(writer.join().unwrap());
    }
}
