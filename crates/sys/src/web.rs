use crate::win::{wide, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, HANDLE};
use ac_core::json::Json;
use ac_core::{bad, Error, Res};
use std::cmp::Ordering;
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};

pub const AGENT: &str = "ACLibriary";

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Res<Json> {
        Json::parse(std::str::from_utf8(&self.body)?)
    }
}

struct H(HANDLE);

impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn h(p: HANDLE, what: &'static str) -> Res<H> {
    if p.is_null() { bad(what) } else { Ok(H(p)) }
}

pub fn split(url: &str) -> Res<(bool, String, u16, String)> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return bad("web: url must start with http:// or https://");
    };
    let (auth, path) = rest.find(['/', '?']).map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let path = if path.starts_with('?') { format!("/{path}") } else { path.to_string() };
    let (host, port) = match auth.rsplit_once(':').filter(|(_, p)| !p.contains(']')) {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| Error::Bad("web: bad port"))?),
        None => (auth, if tls { 443 } else { 80 }),
    };
    if host.is_empty() {
        return bad("web: empty host");
    }
    Ok((tls, host.trim_start_matches('[').trim_end_matches(']').to_string(), port, path))
}

pub struct Req<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: Vec<(&'a str, &'a str)>,
    pub body: &'a [u8],
    pub timeout_ms: i32,
}

impl<'a> Req<'a> {
    pub fn new(method: &'a str, url: &'a str) -> Req<'a> {
        Req { method, url, headers: Vec::new(), body: &[], timeout_ms: 15000 }
    }

    pub fn header(mut self, k: &'a str, v: &'a str) -> Req<'a> {
        self.headers.push((k, v));
        self
    }

    pub fn body(mut self, b: &'a [u8]) -> Req<'a> {
        self.body = b;
        self
    }

    pub fn send(&self) -> Res<Reply> {
        let mut body = Vec::new();
        let status = self.stream(&mut |b| {
            body.extend_from_slice(b);
            Ok(())
        })?;
        Ok(Reply { status, body })
    }

    pub fn stream(&self, sink: &mut dyn FnMut(&[u8]) -> Res<()>) -> Res<u16> {
        let (tls, host, port, path) = split(self.url)?;
        unsafe {
            let s = h(WinHttpOpen(wide(AGENT).as_ptr(), 0, null(), null(), 0), "web: WinHttpOpen failed")?;
            let t = self.timeout_ms;
            WinHttpSetTimeouts(s.0, t, t, t, t);
            let c = h(WinHttpConnect(s.0, wide(&host).as_ptr(), port, 0), "web: connect failed")?;
            let r = h(WinHttpOpenRequest(c.0, wide(self.method).as_ptr(), wide(&path).as_ptr(), null(), null(), null(), if tls { 0x800000 } else { 0 }), "web: open request failed")?;
            let hs: String = self.headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let hw = wide(&hs);
            let n = self.body.len() as u32;
            if WinHttpSendRequest(r.0, if hs.is_empty() { null() } else { hw.as_ptr() }, (hw.len() - 1) as u32, self.body.as_ptr().cast(), n, n, 0) == 0 || WinHttpReceiveResponse(r.0, null_mut()) == 0 {
                return Err(Error::Msg(format!("web: request to {host} failed ({})", crate::win::GetLastError())));
            }
            let (mut code, mut len) = (0u32, 4u32);
            WinHttpQueryHeaders(r.0, 19 | 0x2000_0000, null(), (&mut code as *mut u32).cast(), &mut len, null_mut());
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let mut got = 0u32;
                if WinHttpReadData(r.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut got) == 0 {
                    return bad("web: read failed");
                }
                if got == 0 {
                    break;
                }
                sink(&buf[..got as usize])?;
            }
            Ok(code as u16)
        }
    }
}

pub fn get(url: &str) -> Res<Reply> {
    Req::new("GET", url).send()
}

pub fn download(url: &str, to: &Path, progress: &mut dyn FnMut(u64)) -> Res<u64> {
    let tmp = to.with_extension("part");
    let mut f = std::fs::File::create(&tmp)?;
    let mut n = 0u64;
    let st = Req::new("GET", url).stream(&mut |b| {
        f.write_all(b)?;
        n += b.len() as u64;
        progress(n);
        Ok(())
    });
    drop(f);
    match st {
        Ok(s) if (200..300).contains(&s) => {
            std::fs::rename(&tmp, to)?;
            Ok(n)
        }
        r => {
            let _ = std::fs::remove_file(&tmp);
            Err(r.err().unwrap_or(Error::Msg("web: download failed with http status".into())))
        }
    }
}

pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| -> Vec<u64> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let core = s.split(['-', '+', ' ']).next().unwrap_or("");
        core.split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
    };
    let (x, y) = (parts(a), parts(b));
    (0..x.len().max(y.len())).map(|i| x.get(i).unwrap_or(&0).cmp(y.get(i).unwrap_or(&0))).find(|o| o.is_ne()).unwrap_or(Ordering::Equal)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    pub assets: Vec<(String, String)>,
}

pub fn latest_from(api: &str, owner: &str, repo: &str) -> Res<Release> {
    let r = Req::new("GET", &format!("{api}/repos/{owner}/{repo}/releases/latest")).header("Accept", "application/vnd.github+json").send()?;
    if !r.ok() {
        return Err(Error::Msg(format!("web: github answered {}", r.status)));
    }
    let j = r.json()?;
    let s = |k: &str| j.get(k).and_then(Json::str).unwrap_or("").to_string();
    let assets = j.get("assets").map_or(&[][..], Json::arr).iter().filter_map(|a| Some((a.get("name")?.str()?.to_string(), a.get("browser_download_url")?.str()?.to_string()))).collect();
    let tag = s("tag_name");
    if tag.is_empty() {
        return bad("web: release has no tag");
    }
    Ok(Release { tag, name: s("name"), url: s("html_url"), notes: s("body"), assets })
}

pub fn latest(owner: &str, repo: &str) -> Res<Release> {
    latest_from("https://api.github.com", owner, repo)
}

pub fn newer(current: &str, owner: &str, repo: &str) -> Res<Option<Release>> {
    let r = latest(owner, repo)?;
    Ok((version_cmp(&r.tag, current) == Ordering::Greater).then_some(r))
}

#[cfg(test)]
mod t {
    use super::*;
    use ac_net::http::{Response, Server};

    #[test]
    fn urls_and_versions() {
        assert_eq!(split("https://api.github.com/repos/a/b?x=1").unwrap(), (true, "api.github.com".into(), 443, "/repos/a/b?x=1".into()));
        assert_eq!(split("http://127.0.0.1:8080").unwrap(), (false, "127.0.0.1".into(), 8080, "/".into()));
        assert_eq!(split("http://h?q").unwrap().3, "/?q");
        assert_eq!(split("http://[::1]:81/x").unwrap(), (false, "::1".into(), 81, "/x".into()));
        for b in ["ftp://x", "http://", "http://h:99999/"] {
            assert!(split(b).is_err(), "{b}");
        }
        assert_eq!(version_cmp("v1.2.10", "1.2.9"), Ordering::Greater);
        assert_eq!(version_cmp("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(version_cmp("1.0.0-beta", "1.0.1"), Ordering::Less);
        assert_eq!(version_cmp("V2", "1.99"), Ordering::Greater);
    }

    #[test]
    fn local_server() {
        let srv = Server::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", srv.addr().unwrap());
        let api = base.clone();
        let h = srv
            .spawn(move |r| match r.path().as_str() {
                "/echo" => Response::text(200, &format!("{} {} {}", r.method, r.headers.get("x-test").unwrap_or(""), r.text())),
                "/big" => Response::bytes(200, "application/octet-stream", (0..300_000u32).map(|i| i as u8).collect()),
                "/moved" => Response::redirect(302, "/echo"),
                "/repos/me/tool/releases/latest" => Response::text(200, &format!(r#"{{"tag_name":"v1.3.0","name":"One","html_url":"{api}/r","body":"notes","assets":[{{"name":"a.zip","browser_download_url":"{api}/big"}}]}}"#)),
                _ => Response::text(404, "no"),
            })
            .unwrap();
        let r = Req::new("POST", &format!("{base}/echo")).header("X-Test", "yes").body(b"payload").send().unwrap();
        assert_eq!((r.status, r.text().as_str()), (200, "POST yes payload"));
        assert_eq!(get(&format!("{base}/nothing")).unwrap().status, 404);
        assert_eq!(get(&format!("{base}/moved")).unwrap().text(), "GET  ");
        let big = get(&format!("{base}/big")).unwrap();
        assert_eq!(big.body.len(), 300_000);
        assert!(big.body.iter().enumerate().all(|(i, &b)| b == i as u8));
        let rel = latest_from(&base, "me", "tool").unwrap();
        assert_eq!((rel.tag.as_str(), rel.notes.as_str(), rel.assets.len()), ("v1.3.0", "notes", 1));
        assert!(latest_from(&base, "me", "missing").is_err());
        let p = std::env::temp_dir().join(format!("ac_dl_{}.bin", std::process::id()));
        let mut last = 0;
        assert_eq!(download(&rel.assets[0].1, &p, &mut |n| last = n).unwrap(), 300_000);
        assert_eq!((last, std::fs::metadata(&p).unwrap().len()), (300_000, 300_000));
        std::fs::remove_file(&p).unwrap();
        assert!(download(&format!("{base}/nothing"), &p, &mut |_| {}).is_err());
        assert!(!p.exists() && !p.with_extension("part").exists());
        h.stop();
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert!(get(&format!("http://{free}/")).is_err());
    }

    #[test]
    #[ignore]
    fn github() {
        let r = latest("rust-lang", "rust").unwrap();
        assert!(r.tag.starts_with('1') && r.url.starts_with("https://github.com/"));
    }
}
