use ac_core::json::Json;
use ac_core::{bad, Error, Res};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

#[derive(Clone, Debug, Default)]
pub struct Activity {
    pub details: Option<String>,
    pub state: Option<String>,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub large: Option<(String, String)>,
    pub small: Option<(String, String)>,
    pub party: Option<(u32, u32)>,
    pub buttons: Vec<(String, String)>,
}

fn text(s: &str) -> String {
    let mut t: String = s.chars().take(128).collect();
    while t.chars().count() < 2 {
        t.push(' ');
    }
    t
}

impl Activity {
    pub fn json(&self) -> Json {
        let mut a = Json::obj();
        if let Some(s) = &self.details {
            a = a.set("details", text(s));
        }
        if let Some(s) = &self.state {
            a = a.set("state", text(s));
        }
        if self.start.is_some() || self.end.is_some() {
            let mut t = Json::obj();
            if let Some(s) = self.start {
                t = t.set("start", s);
            }
            if let Some(e) = self.end {
                t = t.set("end", e);
            }
            a = a.set("timestamps", t);
        }
        let mut assets = Json::obj();
        for (k, v) in [("large", &self.large), ("small", &self.small)] {
            if let Some((img, tip)) = v {
                assets = assets.set(&format!("{k}_image"), img.as_str());
                if !tip.is_empty() {
                    assets = assets.set(&format!("{k}_text"), text(tip));
                }
            }
        }
        if assets != Json::obj() {
            a = a.set("assets", assets);
        }
        if let Some((n, m)) = self.party {
            a = a.set("party", Json::obj().set("size", vec![Json::from(n), Json::from(m)]));
        }
        if !self.buttons.is_empty() {
            a = a.set("buttons", self.buttons.iter().take(2).map(|(l, u)| Json::obj().set("label", l.chars().take(32).collect::<String>()).set("url", u.as_str())).collect::<Vec<_>>());
        }
        a
    }
}

pub fn frame(op: u32, j: &Json) -> Vec<u8> {
    let s = j.to_string();
    let mut v = Vec::with_capacity(s.len() + 8);
    v.extend(op.to_le_bytes());
    v.extend((s.len() as u32).to_le_bytes());
    v.extend(s.as_bytes());
    v
}

pub fn read_frame(r: &mut impl Read) -> Res<(u32, Json)> {
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let (op, n) = (u32::from_le_bytes([h[0], h[1], h[2], h[3]]), u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize);
    if n > 1 << 20 {
        return bad("discord: frame too large");
    }
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    Ok((op, Json::parse(std::str::from_utf8(&b)?)?))
}

pub struct Rpc<S: Read + Write = File> {
    s: S,
    nonce: u64,
    pub user: Option<String>,
}

impl Rpc<File> {
    pub fn connect(client: &str) -> Res<Rpc<File>> {
        let mut last = Error::Bad("discord: not running");
        for i in 0..10 {
            match Rpc::connect_at(&format!(r"\\.\pipe\discord-ipc-{i}"), client) {
                Ok(r) => return Ok(r),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub fn connect_at(path: &str, client: &str) -> Res<Rpc<File>> {
        Rpc::handshake(OpenOptions::new().read(true).write(true).open(path)?, client)
    }
}

impl<S: Read + Write> Rpc<S> {
    pub fn handshake(mut s: S, client: &str) -> Res<Rpc<S>> {
        s.write_all(&frame(0, &Json::obj().set("v", 1u32).set("client_id", client)))?;
        let (op, j) = read_frame(&mut s)?;
        if op != 1 || j.get("evt").and_then(Json::str) != Some("READY") {
            return Err(Error::Msg(format!("discord: handshake failed: {}", j.path("message").and_then(Json::str).unwrap_or("unexpected reply"))));
        }
        let user = j.path("data.user.username").and_then(Json::str).map(String::from);
        Ok(Rpc { s, nonce: 0, user })
    }

    fn call(&mut self, args: Json) -> Res<Json> {
        self.nonce += 1;
        let n = self.nonce.to_string();
        self.s.write_all(&frame(1, &Json::obj().set("cmd", "SET_ACTIVITY").set("args", args).set("nonce", n.as_str())))?;
        loop {
            let (op, j) = read_frame(&mut self.s)?;
            match op {
                2 => return bad("discord: connection closed"),
                3 => self.s.write_all(&frame(4, &j))?,
                _ if j.get("nonce").and_then(Json::str) == Some(&n) => {
                    if j.get("evt").and_then(Json::str) == Some("ERROR") {
                        return Err(Error::Msg(format!("discord: {}", j.path("data.message").and_then(Json::str).unwrap_or("error"))));
                    }
                    return Ok(j);
                }
                _ => {}
            }
        }
    }

    pub fn set(&mut self, a: &Activity) -> Res<Json> {
        self.call(Json::obj().set("pid", std::process::id()).set("activity", a.json()))
    }

    pub fn clear(&mut self) -> Res<Json> {
        self.call(Json::obj().set("pid", std::process::id()).set("activity", Json::Null))
    }

    pub fn close(mut self) -> Res<()> {
        self.s.write_all(&frame(2, &Json::obj()))?;
        Ok(())
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::win::{wide, ConnectNamedPipe, CreateNamedPipeW};
    use std::os::windows::io::FromRawHandle;

    #[test]
    fn activity_json() {
        let a = Activity { details: Some("x".into()), state: Some("Paris".into()), start: Some(10), large: Some(("logo".into(), "".into())), party: Some((1, 4)), buttons: vec![("Site".into(), "https://e.x".into())], ..Activity::default() };
        assert_eq!(a.json().to_string(), r#"{"details":"x ","state":"Paris","timestamps":{"start":10},"assets":{"large_image":"logo"},"party":{"size":[1,4]},"buttons":[{"label":"Site","url":"https://e.x"}]}"#);
        assert_eq!(Activity::default().json().to_string(), "{}");
        let f = frame(1, &Json::obj());
        assert_eq!(f, [1, 0, 0, 0, 2, 0, 0, 0, b'{', b'}']);
        assert_eq!(read_frame(&mut &f[..]).unwrap(), (1, Json::obj()));
        assert!(read_frame(&mut &f[..5]).is_err());
    }

    #[test]
    fn talks_to_fake_server() {
        let name = format!(r"\\.\pipe\ac-discord-test-{}", std::process::id());
        let h = unsafe { CreateNamedPipeW(wide(&name).as_ptr(), 3, 0, 1, 4096, 4096, 0, std::ptr::null_mut()) };
        assert!(!h.is_null() && h as isize != -1);
        let h = h as usize;
        let srv = std::thread::spawn(move || {
            let h = h as crate::win::HANDLE;
            unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let mut p = unsafe { File::from_raw_handle(h) };
            let (op, j) = read_frame(&mut p).unwrap();
            assert_eq!((op, j.get("client_id").and_then(Json::str)), (0, Some("123")));
            p.write_all(&frame(1, &Json::parse(r#"{"cmd":"DISPATCH","evt":"READY","data":{"user":{"username":"tester"}}}"#).unwrap())).unwrap();
            let (op, j) = read_frame(&mut p).unwrap();
            assert_eq!(op, 1);
            assert_eq!(j.path("args.activity.state").and_then(Json::str), Some("In menu"));
            assert_eq!(j.path("args.pid").and_then(Json::num), Some(std::process::id() as f64));
            p.write_all(&frame(1, &Json::obj().set("cmd", "DISPATCH").set("evt", "ACTIVITY_JOIN"))).unwrap();
            let n = j.get("nonce").cloned().unwrap();
            p.write_all(&frame(1, &Json::obj().set("cmd", "SET_ACTIVITY").set("nonce", n).set("data", Json::obj()))).unwrap();
            let (_, j) = read_frame(&mut p).unwrap();
            assert!(j.path("args.activity").unwrap().is_null());
            let n = j.get("nonce").cloned().unwrap();
            p.write_all(&frame(1, &Json::parse(r#"{"evt":"ERROR","data":{"message":"nope"}}"#).unwrap().set("nonce", n))).unwrap();
            assert_eq!(read_frame(&mut p).unwrap().0, 2);
        });
        let mut r = Rpc::connect_at(&name, "123").unwrap();
        assert_eq!(r.user.as_deref(), Some("tester"));
        r.set(&Activity { state: Some("In menu".into()), start: Some(now()), ..Activity::default() }).unwrap();
        assert_eq!(r.clear().unwrap_err().to_string(), "discord: nope");
        r.close().unwrap();
        srv.join().unwrap();
        assert!(Rpc::connect_at(r"\\.\pipe\ac-no-such-pipe", "1").is_err());
    }
}
