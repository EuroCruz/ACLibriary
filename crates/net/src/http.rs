use crate::sock::{dial, read_line};
use crate::url::{dec, pairs, Url};
use ac_core::{bad, Error, Res};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const LINE: usize = 16 << 10;
const HEAD: usize = 256 << 10;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers(pub Vec<(String, String)>);

impl Headers {
    pub fn new() -> Headers {
        Headers(Vec::new())
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }

    pub fn all<'a>(&'a self, k: &'a str) -> impl Iterator<Item = &'a str> {
        self.0.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }

    pub fn has(&self, k: &str) -> bool {
        self.get(k).is_some()
    }

    pub fn token(&self, k: &str, t: &str) -> bool {
        self.all(k).flat_map(|v| v.split(',')).any(|x| x.trim().eq_ignore_ascii_case(t))
    }

    pub fn set(&mut self, k: &str, v: impl Into<String>) -> &mut Self {
        self.remove(k);
        self.add(k, v)
    }

    pub fn add(&mut self, k: &str, v: impl Into<String>) -> &mut Self {
        self.0.push((k.to_string(), v.into()));
        self
    }

    pub fn remove(&mut self, k: &str) -> &mut Self {
        self.0.retain(|(n, _)| !n.eq_ignore_ascii_case(k));
        self
    }

    fn put(&self, o: &mut Vec<u8>) {
        for (k, v) in &self.0 {
            o.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub peer: Option<SocketAddr>,
}

impl Request {
    pub fn new(method: &str, target: &str) -> Request {
        Request { method: method.to_string(), target: target.to_string(), version: "HTTP/1.1".into(), ..Request::default() }
    }

    pub fn path(&self) -> String {
        dec(self.target.split(['?', '#']).next().unwrap_or(""), false)
    }

    pub fn query(&self) -> Vec<(String, String)> {
        self.target.split_once('?').map_or(Vec::new(), |(_, q)| pairs(q.split('#').next().unwrap_or("")))
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub version: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

pub fn reason(c: u16) -> &'static str {
    match c {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "",
    }
}

impl Response {
    pub fn new(status: u16) -> Response {
        Response { status, reason: reason(status).into(), version: "HTTP/1.1".into(), headers: Headers::new(), body: Vec::new() }
    }

    pub fn bytes(status: u16, ctype: &str, body: Vec<u8>) -> Response {
        Response::new(status).header("Content-Type", ctype).body(body)
    }

    pub fn text(status: u16, s: &str) -> Response {
        Response::bytes(status, "text/plain; charset=utf-8", s.as_bytes().to_vec())
    }

    pub fn redirect(status: u16, to: &str) -> Response {
        Response::new(status).header("Location", to)
    }

    pub fn header(mut self, k: &str, v: impl Into<String>) -> Response {
        self.headers.set(k, v);
        self
    }

    pub fn body(mut self, b: Vec<u8>) -> Response {
        self.body = b;
        self
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text_body(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn head(r: &mut impl BufRead) -> Res<Option<(String, Headers)>> {
    let start = loop {
        match read_line(r, LINE)? {
            None => return Ok(None),
            Some(l) if l.is_empty() => continue,
            Some(l) => break l,
        }
    };
    let (mut h, mut total) = (Headers::new(), start.len());
    loop {
        let Some(l) = read_line(r, LINE)? else { return bad("connection closed in headers") };
        total += l.len();
        if total > HEAD {
            return bad("headers too large");
        }
        if l.is_empty() {
            return Ok(Some((start, h)));
        }
        if l.starts_with([' ', '\t']) {
            match h.0.last_mut() {
                Some((_, v)) => {
                    v.push(' ');
                    v.push_str(l.trim());
                }
                None => return bad("bad header continuation"),
            }
            continue;
        }
        let Some((k, v)) = l.split_once(':') else { return bad("bad header line") };
        if k.is_empty() || k.contains([' ', '\t']) {
            return bad("bad header name");
        }
        h.add(k, v.trim());
    }
}

fn body(r: &mut impl BufRead, h: &Headers, eof: bool, max: usize) -> Res<Vec<u8>> {
    if h.token("transfer-encoding", "chunked") {
        let mut o = Vec::new();
        loop {
            let Some(l) = read_line(r, LINE)? else { return bad("connection closed in chunked body") };
            let n = usize::from_str_radix(l.split(';').next().unwrap_or("").trim(), 16).map_err(|_| Error::Bad("bad chunk size"))?;
            if n == 0 {
                while read_line(r, LINE)?.is_some_and(|l| !l.is_empty()) {}
                return Ok(o);
            }
            if o.len() + n > max {
                return bad("body exceeds limit");
            }
            let at = o.len();
            o.resize(at + n, 0);
            r.read_exact(&mut o[at..])?;
            read_line(r, LINE)?;
        }
    }
    if let Some(v) = h.get("content-length") {
        let n: usize = v.trim().parse().map_err(|_| Error::Bad("bad content length"))?;
        if n > max {
            return bad("body exceeds limit");
        }
        let mut o = vec![0; n];
        r.read_exact(&mut o)?;
        return Ok(o);
    }
    let mut o = Vec::new();
    if eof {
        r.take(max as u64 + 1).read_to_end(&mut o)?;
        if o.len() > max {
            return bad("body exceeds limit");
        }
    }
    Ok(o)
}

pub struct Client {
    pub timeout: Duration,
    pub redirects: u32,
    pub max_body: usize,
    pub headers: Headers,
    pool: Mutex<HashMap<String, BufReader<TcpStream>>>,
}

impl Default for Client {
    fn default() -> Client {
        let mut headers = Headers::new();
        headers.set("User-Agent", "ac-net/0.1").set("Accept", "*/*");
        Client { timeout: Duration::from_secs(30), redirects: 8, max_body: 1 << 30, headers, pool: Mutex::new(HashMap::new()) }
    }
}

impl Client {
    pub fn new() -> Client {
        Client::default()
    }

    pub fn get(&self, url: &str) -> Res<Response> {
        self.send("GET", url, &Headers::new(), &[])
    }

    pub fn post(&self, url: &str, ctype: &str, body: &[u8]) -> Res<Response> {
        let mut h = Headers::new();
        h.set("Content-Type", ctype);
        self.send("POST", url, &h, body)
    }

    pub fn send(&self, method: &str, url: &str, h: &Headers, body: &[u8]) -> Res<Response> {
        let mut u = Url::parse(url)?;
        let (mut m, mut b) = (method.to_ascii_uppercase(), body.to_vec());
        for _ in 0..=self.redirects {
            let r = self.once(&m, &u, h, &b)?;
            match (r.status, r.headers.get("location")) {
                (301..=303 | 307 | 308, Some(l)) => {
                    u = u.join(l)?;
                    if r.status == 303 || (r.status <= 302 && m == "POST") {
                        m = "GET".into();
                        b.clear();
                    }
                }
                _ => return Ok(r),
            }
        }
        bad("too many redirects")
    }

    fn once(&self, m: &str, u: &Url, h: &Headers, b: &[u8]) -> Res<Response> {
        if u.scheme != "http" {
            return bad("only plain http is supported");
        }
        let key = u.addr();
        let old = self.pool.lock().map_err(|_| Error::Bad("pool poisoned"))?.remove(&key);
        let reused = old.is_some();
        let mut c = match old {
            Some(c) => c,
            None => BufReader::new(dial(&key, self.timeout)?),
        };
        let r = match self.exchange(&mut c, m, u, h, b) {
            Err(_) if reused => {
                c = BufReader::new(dial(&key, self.timeout)?);
                self.exchange(&mut c, m, u, h, b)
            }
            r => r,
        };
        let (resp, keep) = r?;
        if keep {
            if let Ok(mut p) = self.pool.lock() {
                p.insert(key, c);
            }
        }
        Ok(resp)
    }

    fn exchange(&self, c: &mut BufReader<TcpStream>, m: &str, u: &Url, h: &Headers, b: &[u8]) -> Res<(Response, bool)> {
        let mut o = format!("{m} {} HTTP/1.1\r\nHost: {}\r\n", u.target(), u.authority()).into_bytes();
        for (k, v) in &self.headers.0 {
            if !h.has(k) {
                o.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
            }
        }
        h.put(&mut o);
        if !b.is_empty() || matches!(m, "POST" | "PUT" | "PATCH") {
            o.extend_from_slice(format!("Content-Length: {}\r\n", b.len()).as_bytes());
        }
        o.extend_from_slice(b"\r\n");
        o.extend_from_slice(b);
        c.get_mut().write_all(&o)?;
        loop {
            let Some((start, hd)) = head(c)? else { return bad("connection closed") };
            let mut it = start.splitn(3, ' ');
            let version = it.next().unwrap_or("").to_string();
            let status: u16 = it.next().and_then(|s| s.parse().ok()).ok_or(Error::Bad("bad status line"))?;
            if !version.starts_with("HTTP/") {
                return bad("bad status line");
            }
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            let none = m == "HEAD" || status == 204 || status == 304 || (100..200).contains(&status);
            let sized = hd.token("transfer-encoding", "chunked") || hd.has("content-length");
            let close = hd.token("connection", "close") || (version == "HTTP/1.0" && !hd.token("connection", "keep-alive"));
            let body = if none { Vec::new() } else { body(c, &hd, true, self.max_body)? };
            let reason = it.next().unwrap_or("").to_string();
            return Ok((Response { status, reason, version, headers: hd, body }, !close && (none || sized)));
        }
    }
}

pub fn get(url: &str) -> Res<Response> {
    Client::new().get(url)
}

pub type Handler = dyn Fn(&Request) -> Response + Send + Sync;

pub struct Server {
    l: TcpListener,
    pub timeout: Duration,
    pub max_body: usize,
    stop: Arc<AtomicBool>,
}

pub struct Handle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    t: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let Some(t) = self.t.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.halt();
    }
}

impl Server {
    pub fn bind(addr: &str) -> Res<Server> {
        Ok(Server { l: TcpListener::bind(addr)?, timeout: Duration::from_secs(30), max_body: 64 << 20, stop: Arc::new(AtomicBool::new(false)) })
    }

    pub fn addr(&self) -> Res<SocketAddr> {
        Ok(self.l.local_addr()?)
    }

    pub fn run(&self, f: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Res<()> {
        let f: Arc<Handler> = Arc::new(f);
        for s in self.l.incoming() {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok(s) = s else { continue };
            let (f, t, m) = (f.clone(), self.timeout, self.max_body);
            thread::spawn(move || serve(s, &*f, t, m));
        }
        Ok(())
    }

    pub fn spawn(self, f: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Res<Handle> {
        let addr = self.addr()?;
        let stop = self.stop.clone();
        let t = thread::spawn(move || {
            let _ = self.run(f);
        });
        Ok(Handle { addr, stop, t: Some(t) })
    }
}

fn reply(w: &mut impl Write, r: &Response, keep: bool, head_only: bool) -> Res<()> {
    let mut o = format!("HTTP/1.1 {} {}\r\n", r.status, if r.reason.is_empty() { reason(r.status) } else { &r.reason }).into_bytes();
    let mut h = r.headers.clone();
    h.remove("Transfer-Encoding");
    if !(r.status < 200 || r.status == 204 || r.status == 304) {
        h.set("Content-Length", r.body.len().to_string());
    }
    if !keep {
        h.set("Connection", "close");
    }
    h.put(&mut o);
    o.extend_from_slice(b"\r\n");
    if !head_only {
        o.extend_from_slice(&r.body);
    }
    w.write_all(&o)?;
    w.flush()?;
    Ok(())
}

fn serve(s: TcpStream, f: &Handler, timeout: Duration, max: usize) {
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let peer = s.peer_addr().ok();
    let Ok(w) = s.try_clone() else { return };
    let mut w = w;
    let mut r = BufReader::new(s);
    loop {
        let (start, h) = match head(&mut r) {
            Ok(Some(x)) => x,
            Ok(None) => return,
            Err(_) => {
                let _ = reply(&mut w, &Response::text(400, "bad request"), false, false);
                return;
            }
        };
        let p: Vec<&str> = start.split(' ').collect();
        if p.len() != 3 || !p[2].starts_with("HTTP/1.") {
            let _ = reply(&mut w, &Response::text(if p.len() == 3 { 505 } else { 400 }, "bad request line"), false, false);
            return;
        }
        let keep = if p[2] == "HTTP/1.0" { h.token("connection", "keep-alive") } else { !h.token("connection", "close") };
        if h.token("expect", "100-continue") && w.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").is_err() {
            return;
        }
        let b = match body(&mut r, &h, false, max) {
            Ok(b) => b,
            Err(_) => {
                let _ = reply(&mut w, &Response::text(413, "payload too large"), false, false);
                return;
            }
        };
        let req = Request { method: p[0].to_string(), target: p[1].to_string(), version: p[2].to_string(), headers: h, body: b, peer };
        let resp = f(&req);
        let keep = keep && !resp.headers.token("connection", "close");
        if reply(&mut w, &resp, keep, req.method == "HEAD").is_err() || !keep {
            return;
        }
    }
}

pub fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "txt" | "log" | "lua" | "ini" | "cfg" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "dds" => "image/vnd-ms.dds",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

pub fn file(root: &Path, req: &Request) -> Response {
    if req.method != "GET" && req.method != "HEAD" {
        return Response::text(405, "method not allowed").header("Allow", "GET, HEAD");
    }
    let Ok(mut p) = ac_core::fs::join_safe(root, &req.path()) else { return Response::text(403, "forbidden") };
    if p.is_dir() {
        p = p.join("index.html");
    }
    match std::fs::read(&p) {
        Ok(d) => Response::bytes(200, mime(&p.to_string_lossy()), d),
        Err(_) => Response::text(404, "not found"),
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::AtomicUsize;

    fn echo(r: &Request) -> Response {
        match r.path().as_str() {
            "/echo" => {
                let q: Vec<String> = r.query().into_iter().map(|(k, v)| format!("{k}={v}")).collect();
                Response::text(200, &format!("{} {} [{}] {}", r.method, r.path(), q.join(","), r.text())).header("X-Peer", r.peer.map_or(0, |p| p.port()).to_string())
            }
            "/redir" => Response::redirect(302, "/echo?from=redir"),
            "/see" => Response::redirect(303, "echo"),
            "/keep" => Response::redirect(307, "/echo"),
            "/loop" => Response::redirect(301, "/loop"),
            "/big" => Response::bytes(200, "application/octet-stream", (0..300000u32).map(|i| i as u8).collect()),
            "/close" => Response::text(200, "bye").header("Connection", "close"),
            _ => Response::text(404, "nope"),
        }
    }

    #[test]
    #[ignore]
    fn reference_curl() {
        let h = Server::bind("127.0.0.1:0").unwrap().spawn(echo).unwrap();
        let base = format!("http://{}", h.addr);
        let run = |a: &[&str], input: &[u8]| {
            let mut p = std::process::Command::new("curl.exe").args(a).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
            p.stdin.take().unwrap().write_all(input).unwrap();
            String::from_utf8_lossy(&p.wait_with_output().unwrap().stdout).into_owned()
        };
        let o = run(&["-s", "-i", "--data-binary", "hello", &format!("{base}/echo?a=1")], b"");
        assert!(o.starts_with("HTTP/1.1 200 OK") && o.ends_with("POST /echo [a=1] hello"), "{o}");
        let o = run(&["-s", "-T", "-", "-H", "Transfer-Encoding: chunked", &format!("{base}/echo")], b"streamed body");
        assert_eq!(o, "PUT /echo [] streamed body");
        let o = run(&["-s", "-i", &format!("{base}/echo"), &format!("{base}/echo")], b"");
        let peers: Vec<&str> = o.lines().filter_map(|l| l.strip_prefix("X-Peer: ")).collect();
        assert!(peers.len() == 2 && peers[0] == peers[1], "{o}");
        let o = run(&["-s", "-L", &format!("{base}/redir")], b"");
        assert_eq!(o, "GET /echo [from=redir] ");
        let o = run(&["-s", "-o", "NUL", "-w", "%{http_code} %{size_download}", &format!("{base}/big")], b"");
        assert_eq!(o, "200 300000");
        h.stop();
    }

    #[test]
    #[ignore]
    fn reference_server() {
        let Ok(u) = std::env::var("AC_REF_URL") else { return };
        let c = Client::new();
        let a = c.get(&format!("{u}chunked")).unwrap();
        assert_eq!((a.status, a.headers.get("transfer-encoding"), a.text_body().len()), (200, Some("chunked"), 100000));
        assert!(a.text_body().chars().all(|ch| ch == 'z'));
        let b = c.post(&format!("{u}len"), "text/plain", "привет".as_bytes()).unwrap();
        assert_eq!(b.text_body(), "POST /len 12 привет");
        let d = c.get(&format!("{u}missing")).unwrap();
        assert_eq!(d.status, 404);
        let e = c.get(&format!("{u}redirect")).unwrap();
        assert_eq!(e.text_body(), "GET /len 0 ");
    }

    #[test]
    fn client_server() {
        let h = Server::bind("127.0.0.1:0").unwrap().spawn(echo).unwrap();
        let base = format!("http://{}", h.addr);
        let c = Client::new();
        let r = c.get(&format!("{base}/echo?a=1&b=x%20y")).unwrap();
        assert_eq!((r.status, r.text_body().as_str()), (200, "GET /echo [a=1,b=x y] "));
        let port = r.headers.get("x-peer").unwrap().to_string();
        let r = c.post(&format!("{base}/echo"), "text/plain", b"payload").unwrap();
        assert_eq!(r.text_body(), "POST /echo [] payload");
        assert_eq!(r.headers.get("x-peer").unwrap(), port);
        assert_eq!(c.get(&format!("{base}/redir")).unwrap().text_body(), "GET /echo [from=redir] ");
        assert_eq!(c.post(&format!("{base}/see"), "t", b"x").unwrap().text_body(), "GET /echo [] ");
        assert_eq!(c.post(&format!("{base}/keep"), "t", b"kept").unwrap().text_body(), "POST /echo [] kept");
        assert!(c.get(&format!("{base}/loop")).is_err());
        let r = c.get(&format!("{base}/missing")).unwrap();
        assert_eq!((r.status, r.ok(), r.reason.as_str()), (404, false, "Not Found"));
        let big = c.get(&format!("{base}/big")).unwrap();
        assert_eq!(big.body.len(), 300000);
        assert!(big.body.iter().enumerate().all(|(i, &b)| b == i as u8));
        let r = c.send("HEAD", &format!("{base}/big"), &Headers::new(), &[]).unwrap();
        assert_eq!((r.body.len(), r.headers.get("content-length")), (0, Some("300000")));
        assert_eq!(c.get(&format!("{base}/close")).unwrap().text_body(), "bye");
        assert_eq!(c.get(&format!("{base}/echo")).unwrap().status, 200);
        assert!(get("https://example.com/").is_err());
        h.stop();
    }

    #[test]
    fn raw_server_responses() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let t = thread::spawn(move || {
            for (i, s) in l.incoming().take(3).enumerate() {
                let mut s = s.unwrap();
                n2.fetch_add(1, Ordering::SeqCst);
                let mut r = BufReader::new(s.try_clone().unwrap());
                head(&mut r).unwrap();
                let out: &[u8] = match i {
                    0 => b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-Folded: a\r\n  b\r\n\r\n5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\nTrailer: x\r\n\r\n",
                    1 => b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nuntil eof",
                    _ => b"HTTP/1.1 204 No Content\r\n\r\n",
                };
                s.write_all(out).unwrap();
            }
        });
        let c = Client::new();
        let r = c.get(&format!("http://{a}/")).unwrap();
        assert_eq!((r.text_body().as_str(), r.headers.get("x-folded")), ("hello world", Some("a b")));
        assert_eq!(c.get(&format!("http://{a}/")).unwrap().text_body(), "until eof");
        assert_eq!(c.get(&format!("http://{a}/")).unwrap().status, 204);
        t.join().unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn parsing() {
        let mut r = Cursor::new(b"\r\nGET /x HTTP/1.1\r\nA: 1\r\na: 2\r\nConnection: Keep-Alive, Upgrade\r\n\r\n".to_vec());
        let (s, h) = head(&mut r).unwrap().unwrap();
        assert_eq!(s, "GET /x HTTP/1.1");
        assert_eq!(h.all("a").collect::<Vec<_>>(), ["1", "2"]);
        assert!(h.token("connection", "keep-alive") && h.token("CONNECTION", "upgrade"));
        assert!(head(&mut Cursor::new(b"GET / HTTP/1.1\r\nbad\r\n\r\n".to_vec())).is_err());
        assert!(head(&mut Cursor::new(b"GET / HTTP/1.1\r\nA: 1\r\n".to_vec())).is_err());
        let mut hd = Headers::new();
        hd.set("Content-Length", "100");
        assert!(body(&mut Cursor::new(vec![0; 200]), &hd, false, 50).is_err());
        let rq = Request::new("GET", "/a%20b/c?x=1&y=%26#f");
        assert_eq!((rq.path().as_str(), rq.query()), ("/a b/c", vec![("x".into(), "1".into()), ("y".into(), "&".into())]));
        assert_eq!(mime("a/b.PNG"), "image/png");
    }

    #[test]
    fn files_and_errors() {
        let dir = std::env::temp_dir().join(format!("acnet{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/index.html"), "<p>hi</p>").unwrap();
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        let root = dir.clone();
        let h = Server::bind("127.0.0.1:0").unwrap().spawn(move |r| file(&root, r)).unwrap();
        let base = format!("http://{}", h.addr);
        let r = get(&format!("{base}/sub/")).unwrap();
        assert_eq!((r.text_body().as_str(), r.headers.get("content-type")), ("<p>hi</p>", Some("text/html; charset=utf-8")));
        assert_eq!(get(&format!("{base}/a.json")).unwrap().headers.get("content-type"), Some("application/json"));
        assert_eq!(get(&format!("{base}/../x")).unwrap().status, 403);
        assert_eq!(file(&dir, &Request::new("GET", "/\\Windows\\win.ini")).status, 404);
        assert_eq!(file(&dir, &Request::new("GET", "/C:/Windows/win.ini")).status, 403);
        assert_eq!(file(&dir, &Request::new("GET", "/sub\\index.html")).status, 200);
        assert_eq!(get(&format!("{base}/nope")).unwrap().status, 404);
        assert_eq!(Client::new().post(&format!("{base}/a.json"), "t", b"").unwrap().status, 405);
        let mut s = TcpStream::connect(h.addr).unwrap();
        s.write_all(b"BROKEN\r\n\r\n").unwrap();
        let mut o = String::new();
        s.read_to_string(&mut o).unwrap();
        assert!(o.starts_with("HTTP/1.1 400"));
        let mut s = TcpStream::connect(h.addr).unwrap();
        s.write_all(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n3\r\nabc\r\n0\r\n\r\n").unwrap();
        let mut o = String::new();
        s.read_to_string(&mut o).unwrap_or(0);
        assert!(o.starts_with("HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 405"));
        h.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

