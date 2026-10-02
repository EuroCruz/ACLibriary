use crate::pe::Pe;
use ac_core::{bad, Res};

pub const ICON: u32 = 3;
pub const GROUP_ICON: u32 = 14;
pub const VERSION: u32 = 16;
pub const MANIFEST: u32 = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Id {
    Name(String),
    Num(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Dir(Vec<(Id, Node)>),
    Data { bytes: Vec<u8>, code: u32 },
}

impl Node {
    pub fn empty() -> Node {
        Node::Dir(Vec::new())
    }

    pub fn get(&self, path: &[Id]) -> Option<&Node> {
        match (self, path.split_first()) {
            (n, None) => Some(n),
            (Node::Dir(v), Some((h, t))) => v.iter().find(|(k, _)| k == h)?.1.get(t),
            _ => None,
        }
    }

    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Node::Data { bytes, .. } => Some(bytes),
            _ => None,
        }
    }

    pub fn set(&mut self, path: &[Id], n: Node) {
        let Some((h, t)) = path.split_first() else {
            *self = n;
            return;
        };
        if !matches!(self, Node::Dir(_)) {
            *self = Node::empty();
        }
        let Node::Dir(v) = self else { return };
        match v.iter().position(|(k, _)| k == h) {
            Some(i) => v[i].1.set(t, n),
            None => {
                let mut c = Node::empty();
                c.set(t, n);
                v.push((h.clone(), c));
            }
        }
    }

    pub fn remove(&mut self, path: &[Id]) -> bool {
        let Node::Dir(v) = self else { return false };
        match path {
            [] => false,
            [h] => v.iter().position(|(k, _)| k == h).map(|i| v.remove(i)).is_some(),
            [h, t @ ..] => v.iter_mut().find(|(k, _)| k == h).is_some_and(|(_, c)| c.remove(t)),
        }
    }

    pub fn ids(&self) -> Vec<Id> {
        match self {
            Node::Dir(v) => v.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }
}

fn sorted(v: &[(Id, Node)]) -> Vec<&(Id, Node)> {
    let mut o: Vec<&(Id, Node)> = v.iter().collect();
    o.sort_by(|a, b| match (&a.0, &b.0) {
        (Id::Name(x), Id::Name(y)) => x.to_uppercase().cmp(&y.to_uppercase()),
        (Id::Name(_), Id::Num(_)) => std::cmp::Ordering::Less,
        (Id::Num(_), Id::Name(_)) => std::cmp::Ordering::Greater,
        (Id::Num(x), Id::Num(y)) => x.cmp(y),
    });
    o
}

struct Plan {
    dirs: usize,
    leaves: usize,
    strs: usize,
    data: usize,
}

fn plan(n: &Node, p: &mut Plan) {
    match n {
        Node::Dir(v) => {
            p.dirs += 16 + 8 * v.len();
            for (k, c) in v {
                if let Id::Name(s) = k {
                    p.strs += 2 + s.encode_utf16().count() * 2;
                }
                plan(c, p);
            }
        }
        Node::Data { bytes, .. } => {
            p.leaves += 16;
            p.data += bytes.len().div_ceil(4) * 4;
        }
    }
}

struct Emit {
    buf: Vec<u8>,
    rva: u32,
    dc: usize,
    lc: usize,
    sc: usize,
    bc: usize,
}

impl Emit {
    fn w32(&mut self, o: usize, v: u32) {
        self.buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn dir(&mut self, v: &[(Id, Node)], at: usize) {
        let items = sorted(v);
        let named = items.iter().filter(|(k, _)| matches!(k, Id::Name(_))).count();
        self.buf[at + 12..at + 14].copy_from_slice(&(named as u16).to_le_bytes());
        self.buf[at + 14..at + 16].copy_from_slice(&((items.len() - named) as u16).to_le_bytes());
        let mut subs = Vec::new();
        for (i, (k, c)) in items.iter().enumerate() {
            let e = at + 16 + i * 8;
            let key = match k {
                Id::Num(n) => *n,
                Id::Name(s) => {
                    let o = self.sc;
                    let u: Vec<u16> = s.encode_utf16().collect();
                    self.buf[o..o + 2].copy_from_slice(&(u.len() as u16).to_le_bytes());
                    for (j, w) in u.iter().enumerate() {
                        self.buf[o + 2 + j * 2..o + 4 + j * 2].copy_from_slice(&w.to_le_bytes());
                    }
                    self.sc += 2 + u.len() * 2;
                    0x8000_0000 | o as u32
                }
            };
            self.w32(e, key);
            match c {
                Node::Dir(cv) => {
                    let o = self.dc;
                    self.dc += 16 + 8 * cv.len();
                    self.w32(e + 4, 0x8000_0000 | o as u32);
                    subs.push((cv, o));
                }
                Node::Data { bytes, code } => {
                    let (l, b) = (self.lc, self.bc);
                    self.lc += 16;
                    self.bc += bytes.len().div_ceil(4) * 4;
                    self.w32(e + 4, l as u32);
                    self.w32(l, self.rva + b as u32);
                    self.w32(l + 4, bytes.len() as u32);
                    self.w32(l + 8, *code);
                    self.buf[b..b + bytes.len()].copy_from_slice(bytes);
                }
            }
        }
        for (cv, o) in subs {
            self.dir(cv, o);
        }
    }
}

pub fn serialize(root: &Node, rva: u32) -> Res<Vec<u8>> {
    let Node::Dir(v) = root else { return bad("resource root must be a directory") };
    let mut p = Plan { dirs: 0, leaves: 0, strs: 0, data: 0 };
    plan(root, &mut p);
    let strs = p.strs.div_ceil(4) * 4;
    let total = p.dirs + p.leaves + strs + p.data;
    let mut e = Emit { buf: vec![0; total], rva, dc: 16 + 8 * v.len(), lc: p.dirs, sc: p.dirs + p.leaves, bc: p.dirs + p.leaves + strs };
    let rva_base = rva;
    e.rva = rva_base;
    let leaf_rva_fix = 0;
    let _ = leaf_rva_fix;
    e.dir(v, 0);
    Ok(e.buf)
}

fn read_dir(pe: &Pe, base: u32, off: u32, depth: u32) -> Option<Node> {
    if depth > 4 {
        return None;
    }
    let h = pe.slice(base + off, 16)?;
    let n = u16::from_le_bytes([h[12], h[13]]) as u32 + u16::from_le_bytes([h[14], h[15]]) as u32;
    if n > 4096 {
        return None;
    }
    let mut v = Vec::new();
    for i in 0..n {
        let e = pe.slice(base + off + 16 + i * 8, 8)?;
        let (k, o) = (u32::from_le_bytes(e[..4].try_into().unwrap()), u32::from_le_bytes(e[4..].try_into().unwrap()));
        let id = if k & 0x8000_0000 != 0 {
            let s = pe.slice(base + (k & 0x7fff_ffff), 2)?;
            let len = u16::from_le_bytes([s[0], s[1]]) as usize;
            let u = pe.slice(base + (k & 0x7fff_ffff) + 2, len * 2)?;
            Id::Name(String::from_utf16_lossy(&u.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
        } else {
            Id::Num(k)
        };
        let node = if o & 0x8000_0000 != 0 {
            read_dir(pe, base, o & 0x7fff_ffff, depth + 1)?
        } else {
            let d = pe.slice(base + o, 16)?;
            let (rva, size, code) = (u32::from_le_bytes(d[..4].try_into().unwrap()), u32::from_le_bytes(d[4..8].try_into().unwrap()), u32::from_le_bytes(d[8..12].try_into().unwrap()));
            Node::Data { bytes: pe.slice(rva, size as usize)?.to_vec(), code }
        };
        v.push((id, node));
    }
    Some(Node::Dir(v))
}

impl Pe {
    pub fn resources(&self) -> Option<Node> {
        let (rva, size) = self.dir(2);
        if rva == 0 || size == 0 {
            return None;
        }
        read_dir(self, rva, 0, 0)
    }

    pub fn set_resources(&mut self, root: &Node) -> Res<()> {
        let (rva, _) = self.dir(2);
        let s = self.sections();
        let host = (rva != 0).then(|| s.iter().position(|x| x.va == rva)).flatten();
        match host {
            Some(i) if i + 1 == s.len() => {
                let data = serialize(root, rva)?;
                let sec = &s[i];
                let (fa, sa) = (self.file_align().max(1), self.sect_align().max(1));
                let raw = (data.len() as u32).div_ceil(fa) * fa;
                let end = (sec.ptr + sec.rsize) as usize;
                let tail = self.d.split_off(end.min(self.d.len()));
                self.d.truncate(sec.ptr as usize);
                self.d.extend_from_slice(&data);
                self.d.resize((sec.ptr + raw) as usize, 0);
                self.d.extend_from_slice(&tail);
                let o = self.sec_table() + i * 40;
                self.w32(o + 8, data.len() as u32);
                self.w32(o + 16, raw);
                let _ = sa;
                self.set_dir(2, rva, data.len() as u32);
            }
            Some(_) => return bad("resource section is not the last section"),
            None => {
                let sa = self.sect_align().max(1);
                let va = s.iter().map(|x| x.end_va()).max().unwrap_or(0).max(self.headers_size()).div_ceil(sa) * sa;
                let data = serialize(root, va)?;
                self.add_section(".rsrc", &data, 0x4000_0040)?;
                self.set_dir(2, va, data.len() as u32);
            }
        }
        self.refresh();
        Ok(())
    }

    pub fn set_icon(&mut self, ico: &[u8]) -> Res<()> {
        let u16at = |o: usize| ico.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let u32at = |o: usize| ico.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        if u16at(0) != Some(0) || u16at(2) != Some(1) {
            return bad("not an ico file");
        }
        let n = u16at(4).ok_or(ac_core::Error::Bad("truncated ico"))? as usize;
        let mut root = self.resources().unwrap_or_else(Node::empty);
        let old = root.get(&[Id::Num(GROUP_ICON)]).map(Node::ids).unwrap_or_default();
        let gid = old.iter().find_map(|k| if let Id::Num(n) = k { Some(*n) } else { None }).unwrap_or(1);
        let lang = match root.get(&[Id::Num(GROUP_ICON), Id::Num(gid)]).map(Node::ids).and_then(|v| v.into_iter().next()) {
            Some(Id::Num(l)) => l,
            _ => 1033,
        };
        root.remove(&[Id::Num(ICON)]);
        root.remove(&[Id::Num(GROUP_ICON)]);
        let mut grp = vec![0, 0, 1, 0];
        grp.extend((n as u16).to_le_bytes());
        for i in 0..n {
            let e = 6 + i * 16;
            let (size, off) = (u32at(e + 8).ok_or(ac_core::Error::Bad("truncated ico"))? as usize, u32at(e + 12).unwrap_or(0) as usize);
            let img = ico.get(off..off + size).ok_or(ac_core::Error::Bad("ico image outside file"))?;
            let id = i as u32 + 1;
            root.set(&[Id::Num(ICON), Id::Num(id), Id::Num(lang)], Node::Data { bytes: img.to_vec(), code: 0 });
            grp.extend(&ico[e..e + 12]);
            grp.extend((id as u16).to_le_bytes());
        }
        root.set(&[Id::Num(GROUP_ICON), Id::Num(gid), Id::Num(lang)], Node::Data { bytes: grp, code: 0 });
        self.set_resources(&root)
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::sample;

    fn tree() -> Node {
        let mut n = Node::empty();
        n.set(&[Id::Num(VERSION), Id::Num(1), Id::Num(1033)], Node::Data { bytes: b"ver".to_vec(), code: 0 });
        n.set(&[Id::Num(MANIFEST), Id::Num(1), Id::Num(1033)], Node::Data { bytes: b"<m/>x".to_vec(), code: 0 });
        n.set(&[Id::Name("DATA".into()), Id::Name("blob".into()), Id::Num(0)], Node::Data { bytes: vec![9; 7], code: 1252 });
        n
    }

    #[test]
    fn tree_ops() {
        let mut n = tree();
        assert_eq!(n.get(&[Id::Num(VERSION), Id::Num(1), Id::Num(1033)]).and_then(Node::data), Some(&b"ver"[..]));
        assert!(n.remove(&[Id::Num(VERSION)]));
        assert!(!n.remove(&[Id::Num(VERSION)]));
        assert!(n.get(&[Id::Num(VERSION)]).is_none());
        assert_eq!(n.ids().len(), 2);
    }

    #[test]
    fn roundtrip_through_pe() {
        let mut p = Pe::parse(sample()).unwrap();
        assert!(p.resources().is_none());
        p.set_resources(&tree()).unwrap();
        let r = p.resources().unwrap();
        assert_eq!(r.get(&[Id::Name("DATA".into()), Id::Name("blob".into()), Id::Num(0)]), tree().get(&[Id::Name("DATA".into()), Id::Name("blob".into()), Id::Num(0)]));
        assert_eq!(r.get(&[Id::Num(MANIFEST), Id::Num(1), Id::Num(1033)]).and_then(Node::data), Some(&b"<m/>x"[..]));
        let again = Pe::parse(p.bytes().to_vec()).unwrap();
        assert_eq!(again.resources(), Some(r.clone()));
        let mut bigger = r.clone();
        bigger.set(&[Id::Num(VERSION), Id::Num(1), Id::Num(1033)], Node::Data { bytes: vec![1; 3000], code: 0 });
        p.set_resources(&bigger).unwrap();
        assert_eq!(p.resources(), Some(bigger));
        assert_eq!(p.sections().len(), 3);
        assert_eq!(p.image_size() % 0x1000, 0);
    }

    #[test]
    fn icon_replacement() {
        let mut ico = vec![0, 0, 1, 0, 2, 0];
        for (w, size, off) in [(32u8, 4u32, 38u32), (16, 2, 42)] {
            ico.extend([w, w, 0, 0, 1, 0, 32, 0]);
            ico.extend(size.to_le_bytes());
            ico.extend(off.to_le_bytes());
        }
        ico.extend([1, 2, 3, 4, 5, 6]);
        let mut p = Pe::parse(sample()).unwrap();
        p.set_icon(&ico).unwrap();
        let r = p.resources().unwrap();
        assert_eq!(r.get(&[Id::Num(ICON), Id::Num(1), Id::Num(1033)]).and_then(Node::data), Some(&[1, 2, 3, 4][..]));
        assert_eq!(r.get(&[Id::Num(ICON), Id::Num(2), Id::Num(1033)]).and_then(Node::data), Some(&[5, 6][..]));
        let g = r.get(&[Id::Num(GROUP_ICON), Id::Num(1), Id::Num(1033)]).and_then(Node::data).unwrap();
        assert_eq!(g.len(), 6 + 14 * 2);
        assert_eq!(g[0..6], [0, 0, 1, 0, 2, 0]);
        assert_eq!(u16::from_le_bytes([g[6 + 12], g[6 + 13]]), 1);
        p.set_icon(&ico).unwrap();
        assert_eq!(p.resources().unwrap().ids(), vec![Id::Num(ICON), Id::Num(GROUP_ICON)]);
        assert!(p.set_icon(&[1, 2, 3]).is_err());
    }
}
