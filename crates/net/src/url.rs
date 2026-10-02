use ac_core::{bad, Res};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Url {
    pub scheme: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: String,
    pub frag: String,
}

pub fn default_port(scheme: &str) -> u16 {
    match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "ftp" => 21,
        _ => 0,
    }
}

fn unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"-._~".contains(&c)
}

fn esc(s: &str, keep: &[u8]) -> String {
    let mut o = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if unreserved(c) || keep.contains(&c) {
            o.push(c as char);
        } else {
            o.push_str(&format!("%{c:02X}"));
        }
    }
    o
}

pub fn enc(s: &str) -> String {
    esc(s, b"")
}

pub fn enc_path(s: &str) -> String {
    esc(s, b"/!$&'()*+,;=:@%")
}

pub fn dec(s: &str, plus: bool) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let hex = |c: u8| (c as char).to_digit(16);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    o.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                _ => o.push(b'%'),
            },
            b'+' if plus => o.push(b' '),
            c => o.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

pub fn query(p: &[(&str, &str)]) -> String {
    p.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

pub fn pairs(q: &str) -> Vec<(String, String)> {
    q.split('&').filter(|s| !s.is_empty()).map(|s| s.split_once('=').map_or((dec(s, true), String::new()), |(k, v)| (dec(k, true), dec(v, true)))).collect()
}

impl Url {
    pub fn parse(s: &str) -> Res<Url> {
        let s = s.trim();
        let (scheme, rest) = match s.find("://") {
            Some(i) if s[..i].bytes().all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c)) && i > 0 => (s[..i].to_ascii_lowercase(), &s[i + 3..]),
            _ => ("http".to_string(), s),
        };
        let (rest, frag) = rest.split_once('#').unwrap_or((rest, ""));
        let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (auth, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let (user, hp) = auth.rsplit_once('@').unwrap_or(("", auth));
        let (host, port) = if let Some(h) = hp.strip_prefix('[') {
            let Some((h, r)) = h.split_once(']') else { return bad("bad ipv6 host") };
            (h, r.strip_prefix(':'))
        } else {
            match hp.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (hp, None),
            }
        };
        if host.is_empty() {
            return bad("missing host");
        }
        let port = match port {
            Some(p) if !p.is_empty() => p.parse().map_err(|_| ac_core::Error::Bad("bad port"))?,
            _ => default_port(&scheme),
        };
        Ok(Url { scheme, user: user.to_string(), host: host.to_ascii_lowercase(), port, path: path.to_string(), query: query.to_string(), frag: frag.to_string() })
    }

    pub fn target(&self) -> String {
        let p = if self.path.is_empty() { "/" } else { &self.path };
        if self.query.is_empty() { p.to_string() } else { format!("{p}?{}", self.query) }
    }

    pub fn authority(&self) -> String {
        let h = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == default_port(&self.scheme) { h } else { format!("{h}:{}", self.port) }
    }

    pub fn addr(&self) -> String {
        let h = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        format!("{h}:{}", self.port)
    }

    pub fn join(&self, r: &str) -> Res<Url> {
        if r.contains("://") {
            return Url::parse(r);
        }
        if let Some(x) = r.strip_prefix("//") {
            return Url::parse(&format!("{}://{x}", self.scheme));
        }
        let mut u = self.clone();
        u.frag.clear();
        let (r, frag) = r.split_once('#').unwrap_or((r, ""));
        u.frag = frag.to_string();
        if r.is_empty() {
            return Ok(u);
        }
        let (p, q) = r.split_once('?').map_or((r, None), |(p, q)| (p, Some(q)));
        u.query = q.unwrap_or("").to_string();
        if p.is_empty() {
            if q.is_none() {
                u.query = self.query.clone();
            }
            return Ok(u);
        }
        let base = if p.starts_with('/') { String::new() } else { self.path[..self.path.rfind('/').map_or(0, |i| i + 1)].to_string() };
        let mut seg: Vec<&str> = Vec::new();
        let full = format!("{base}{p}");
        let parts: Vec<&str> = full.split('/').collect();
        for (i, s) in parts.iter().enumerate() {
            match *s {
                "." => {}
                ".." => {
                    if seg.len() > 1 {
                        seg.pop();
                    }
                }
                _ => seg.push(s),
            }
            if i + 1 == parts.len() && (*s == "." || *s == "..") {
                seg.push("");
            }
        }
        u.path = seg.join("/");
        if !u.path.starts_with('/') {
            u.path.insert(0, '/');
        }
        Ok(u)
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}://", self.scheme)?;
        if !self.user.is_empty() {
            write!(f, "{}@", self.user)?;
        }
        write!(f, "{}{}", self.authority(), if self.path.is_empty() { "/" } else { &self.path })?;
        if !self.query.is_empty() {
            write!(f, "?{}", self.query)?;
        }
        if !self.frag.is_empty() {
            write!(f, "#{}", self.frag)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn parse() {
        let u = Url::parse("HTTP://user:pw@Example.com:8080/a/b?x=1&y=2#top").unwrap();
        assert_eq!((u.scheme.as_str(), u.user.as_str(), u.host.as_str(), u.port), ("http", "user:pw", "example.com", 8080));
        assert_eq!((u.path.as_str(), u.query.as_str(), u.frag.as_str()), ("/a/b", "x=1&y=2", "top"));
        assert_eq!(u.target(), "/a/b?x=1&y=2");
        assert_eq!(u.to_string(), "http://user:pw@example.com:8080/a/b?x=1&y=2#top");
        let v = Url::parse("example.org").unwrap();
        assert_eq!((v.port, v.target().as_str(), v.authority().as_str()), (80, "/", "example.org"));
        let w = Url::parse("https://[::1]:9000/x").unwrap();
        assert_eq!((w.host.as_str(), w.port, w.addr().as_str()), ("::1", 9000, "[::1]:9000"));
        assert_eq!(Url::parse("https://h/").unwrap().port, 443);
        assert!(Url::parse("http://:80/").is_err());
        assert!(Url::parse("http://h:x/").is_err());
    }

    #[test]
    fn join() {
        let b = Url::parse("http://h/a/b/c?q=1").unwrap();
        let j = |r: &str| b.join(r).unwrap().to_string();
        assert_eq!(j("d"), "http://h/a/b/d");
        assert_eq!(j("../d"), "http://h/a/d");
        assert_eq!(j("../../../../d"), "http://h/d");
        assert_eq!(j("./"), "http://h/a/b/");
        assert_eq!(j("/x?y=2"), "http://h/x?y=2");
        assert_eq!(j("?z"), "http://h/a/b/c?z");
        assert_eq!(j("#f"), "http://h/a/b/c?q=1#f");
        assert_eq!(j("//o:81/p"), "http://o:81/p");
        assert_eq!(j("https://s/t"), "https://s/t");
        assert_eq!(j(".."), "http://h/a/");
    }

    #[test]
    fn coding() {
        assert_eq!(enc("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(dec("a%20b+c%C3%A9%zz%4", true), "a b cé%zz%4");
        assert_eq!(dec("a+b", false), "a+b");
        assert_eq!(enc_path("/a b/c"), "/a%20b/c");
        assert_eq!(query(&[("k", "v 1"), ("x", "&")]), "k=v%201&x=%26");
        assert_eq!(pairs("k=v+1&e&x=%26"), [("k".into(), "v 1".into()), ("e".into(), String::new()), ("x".into(), "&".into())]);
    }
}
