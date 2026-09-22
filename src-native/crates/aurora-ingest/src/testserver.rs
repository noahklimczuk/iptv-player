//! A tiny blocking HTTP server for tests.
//!
//! Hand-rolled rather than pulled from a mocking crate because README §20 asks for
//! "simulated failures: 401, 403, 404, 429, slow-loris, mid-stream disconnect", and
//! cutting a connection in the middle of a body is exactly what canned mock servers
//! will not do for you. It is also blocking, which matches the client under test.

#![cfg(test)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// What the server should do with one request.
#[derive(Debug, Clone)]
pub enum Reply {
    /// A normal response.
    Body {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Announce a Content-Length, send part of the body, then drop the socket.
    Truncated { announced: usize, send: Vec<u8> },
    /// Accept the connection and never answer.
    Hang,
    /// Close immediately, before any status line.
    Reset,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Reply::Body { status: 200, headers: Vec::new(), body: body.into() }
    }

    pub fn status(status: u16) -> Self {
        Reply::Body { status, headers: Vec::new(), body: b"error".to_vec() }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        if let Reply::Body { headers, .. } = &mut self {
            headers.push((name.to_string(), value.to_string()));
        }
        self
    }
}

/// One received request, for assertions.
#[derive(Debug, Clone, Default)]
pub struct Received {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct TestServer {
    port: u16,
    stop: Arc<AtomicBool>,
    pub hits: Arc<AtomicUsize>,
    pub log: Arc<std::sync::Mutex<Vec<Received>>>,
}

impl TestServer {
    /// Start a server whose responder is called once per request, with a zero-based
    /// request index so a test can fail twice and then succeed.
    pub fn start<F>(responder: F) -> Self
    where
        F: Fn(usize, &Received) -> Reply + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        listener.set_nonblocking(true).expect("nonblocking");

        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let log: Arc<std::sync::Mutex<Vec<Received>>> = Arc::default();

        let (s, h, l) = (stop.clone(), hits.clone(), log.clone());
        let responder = Arc::new(responder);

        thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (h, l, responder) = (h.clone(), l.clone(), responder.clone());
                        thread::spawn(move || {
                            let _ = handle(stream, h, l, responder);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self { port, stop, hits, log }
    }

    /// Always answer the same way.
    pub fn always(reply: Reply) -> Self {
        Self::start(move |_, _| reply.clone())
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn request_count(&self) -> usize {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn requests(&self) -> Vec<Received> {
        self.log.lock().expect("log").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn handle<F>(
    mut stream: TcpStream,
    hits: Arc<AtomicUsize>,
    log: Arc<std::sync::Mutex<Vec<Received>>>,
    responder: Arc<F>,
) -> std::io::Result<()>
where
    F: Fn(usize, &Received) -> Reply + Send + Sync + 'static,
{
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }

    let mut parts = line.split_whitespace();
    let mut req = Received {
        method: parts.next().unwrap_or_default().to_string(),
        path: parts.next().unwrap_or_default().to_string(),
        headers: Vec::new(),
    };

    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            req.headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    let index = hits.fetch_add(1, Ordering::Relaxed);
    log.lock().expect("log").push(req.clone());

    match responder(index, &req) {
        Reply::Body { status, headers, body } => {
            let mut head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                reason(status),
                body.len()
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes())?;
            stream.write_all(&body)?;
            stream.flush()?;
        }
        Reply::Truncated { announced, send } => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {announced}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes())?;
            stream.write_all(&send)?;
            stream.flush()?;
            // Drop without sending the rest: the client sees a short read.
            stream.shutdown(std::net::Shutdown::Both).ok();
        }
        Reply::Hang => {
            // Hold the connection open without replying, until the client times out.
            let mut sink = [0u8; 1];
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
            let _ = stream.read(&mut sink);
        }
        Reply::Reset => {
            stream.shutdown(std::net::Shutdown::Both).ok();
        }
    }
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}
