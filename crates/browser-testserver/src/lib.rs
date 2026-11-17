//! A tiny, dependency-free HTTP server for deterministic browser tests.
//!
//! Tests must never depend on third-party websites: this server exposes fixed
//! pages that exercise HTML, CSS, JavaScript, forms, redirects, downloads and
//! error status codes on a loopback address.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A running test server bound to a random loopback port.
pub struct TestServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    /// Starts the server on `127.0.0.1:0`.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_thread = Arc::clone(&shutdown);

        let handle = thread::spawn(move || {
            while !shutdown_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        thread::spawn(move || {
                            let _ = handle_connection(stream);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr,
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    /// Blocks until the server stops accepting connections.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
    }
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/").to_owned();

    // Drain headers.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }

    let (status, content_type, body, extra_headers) = route(&path);
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn route(path: &str) -> (&'static str, &'static str, String, &'static str) {
    match path.split('?').next().unwrap_or("/") {
        "/" => ("200 OK", "text/html; charset=utf-8", INDEX.to_owned(), ""),
        "/css" => ("200 OK", "text/css", "h1 { color: rgb(12, 120, 90); }".to_owned(), ""),
        "/js" => (
            "200 OK",
            "application/javascript",
            "window.__testLoaded = true; document.addEventListener('DOMContentLoaded', function () { document.getElementById('js').textContent = 'js-ok'; });"
                .to_owned(),
            "",
        ),
        "/page2" => ("200 OK", "text/html; charset=utf-8", PAGE_TWO.to_owned(), ""),
        "/redirect" => (
            "302 Found",
            "text/plain",
            "redirecting".to_owned(),
            "Location: /page2\r\n",
        ),
        "/download" => (
            "200 OK",
            "application/octet-stream",
            "download-payload".to_owned(),
            "Content-Disposition: attachment; filename=\"payload.txt\"\r\n",
        ),
        "/slow" => {
            thread::sleep(Duration::from_millis(800));
            ("200 OK", "text/html; charset=utf-8", PAGE_TWO.to_owned(), "")
        }
        "/favicon.ico" => ("404 Not Found", "text/plain", "no icon".to_owned(), ""),
        _ => ("404 Not Found", "text/html; charset=utf-8", NOT_FOUND.to_owned(), ""),
    }
}

const INDEX: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Local Fixture</title>
<link rel="stylesheet" href="/css">
<script src="/js"></script>
<style>body { margin: 0; font-family: system-ui; } #box { width: 200px; height: 60px; background: rgb(20, 30, 40); }</style>
</head>
<body><h1 id="heading">initial-heading</h1>
<p id="js">no-js</p>
<div id="box"></div>
<a id="next" href="/page2">next page</a>
<form action="/page2" method="get"><input name="q" value="search-term"><button type="submit">Go</button></form>
<script>document.getElementById('heading').textContent = 'fixture-heading'; window.__fixture = { answer: 42 };</script>
</body></html>"#;

const PAGE_TWO: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Second Page</title></head>
<body><h1>page-two</h1><a href="/">back</a></body></html>"#;

const NOT_FOUND: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Not Found</title></head>
<body><h1>404</h1></body></html>"#;

#[cfg(test)]
mod tests {
    use super::TestServer;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn get(server: &TestServer, path: &str) -> String {
        let mut stream = TcpStream::connect(server.addr()).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn serves_index_and_404() {
        let server = TestServer::start().expect("start server");
        let index = get(&server, "/");
        assert!(index.contains("200 OK"));
        assert!(index.contains("fixture-heading"));
        let missing = get(&server, "/nope");
        assert!(missing.contains("404 Not Found"));
    }

    #[test]
    fn serves_redirects_and_downloads() {
        let server = TestServer::start().expect("start server");
        let redirect = get(&server, "/redirect");
        assert!(redirect.contains("302 Found"));
        assert!(redirect.contains("Location: /page2"));
        let download = get(&server, "/download");
        assert!(download.contains("attachment"));
    }
}
