use ac_core::{bad, Res};

fn table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 { r << 1 ^ 0x04c1_1db7 } else { r << 1 };
            }
            *v = r;
        }
        t
    })
}

pub fn crc(d: &[u8]) -> u32 {
    let t = table();
    d.iter().fold(0u32, |c, &b| c << 8 ^ t[((c >> 24) as u8 ^ b) as usize])
}

#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub data: Vec<u8>,
    pub granule: i64,
    pub last: bool,
}

pub fn packets(d: &[u8]) -> Res<Vec<Packet>> {
    let (mut out, mut cur, mut i) = (Vec::new(), Vec::new(), 0usize);
    let mut serial = None;
    let mut started = false;
    while i + 27 <= d.len() {
        if &d[i..i + 4] != b"OggS" {
            i += 1;
            continue;
        }
        let nseg = d[i + 26] as usize;
        let hl = 27 + nseg;
        if i + hl > d.len() {
            break;
        }
        let lace = &d[i + 27..i + hl];
        let body: usize = lace.iter().map(|&x| x as usize).sum();
        if i + hl + body > d.len() {
            break;
        }
        let mut page = d[i..i + hl + body].to_vec();
        let want = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
        page[22..26].fill(0);
        if crc(&page) != want {
            i += 1;
            continue;
        }
        let flags = d[i + 5];
        let granule = i64::from_le_bytes(d[i + 6..i + 14].try_into().unwrap());
        let sn = u32::from_le_bytes(d[i + 14..i + 18].try_into().unwrap());
        if serial.is_none() && flags & 2 != 0 {
            serial = Some(sn);
        }
        if serial == Some(sn) {
            if flags & 1 == 0 && !cur.is_empty() {
                cur.clear();
            }
            let mut off = i + hl;
            let end = lace.iter().rposition(|&x| x < 255);
            let mut skip = flags & 1 != 0 && !started;
            for (k, &l) in lace.iter().enumerate() {
                if !skip {
                    cur.extend_from_slice(&d[off..off + l as usize]);
                }
                off += l as usize;
                if l < 255 {
                    if skip {
                        skip = false;
                        continue;
                    }
                    let fin = end == Some(k);
                    out.push(Packet { data: std::mem::take(&mut cur), granule: if fin { granule } else { -1 }, last: fin && flags & 4 != 0 });
                }
            }
            started = true;
            if flags & 4 != 0 {
                break;
            }
        }
        i += hl + body;
    }
    if out.is_empty() {
        return bad("ogg: no packets");
    }
    Ok(out)
}

pub struct Writer {
    pub out: Vec<u8>,
    serial: u32,
    seq: u32,
    lace: Vec<u8>,
    body: Vec<u8>,
    granule: i64,
    first: bool,
    cont: bool,
    done: bool,
    last: usize,
}

impl Writer {
    pub fn new(serial: u32) -> Writer {
        Writer { out: Vec::new(), serial, seq: 0, lace: Vec::new(), body: Vec::new(), granule: 0, first: true, cont: false, done: false, last: 0 }
    }

    fn page(&mut self, eos: bool) {
        if self.lace.is_empty() && !eos {
            return;
        }
        let mut p = Vec::with_capacity(27 + self.lace.len() + self.body.len());
        p.extend(b"OggS\0");
        p.push(self.cont as u8 | if self.first { 2 } else { 0 } | if eos { 4 } else { 0 });
        p.extend(if self.done || eos { self.granule } else { -1 }.to_le_bytes());
        p.extend(self.serial.to_le_bytes());
        p.extend(self.seq.to_le_bytes());
        p.extend([0; 4]);
        p.push(self.lace.len() as u8);
        p.extend(&self.lace);
        p.extend(&self.body);
        let c = crc(&p);
        p[22..26].copy_from_slice(&c.to_le_bytes());
        self.last = self.out.len();
        self.out.extend(p);
        self.seq += 1;
        self.first = false;
        self.cont = false;
        self.done = false;
        self.lace.clear();
        self.body.clear();
    }

    pub fn packet(&mut self, d: &[u8], granule: i64, flush: bool) {
        let mut rest = d;
        loop {
            let n = rest.len().min(255);
            if self.lace.len() == 255 {
                self.page(false);
                self.cont = true;
            }
            self.lace.push(n as u8);
            self.body.extend_from_slice(&rest[..n]);
            rest = &rest[n..];
            if n < 255 {
                break;
            }
        }
        self.granule = granule;
        self.done = true;
        if flush || self.body.len() >= 4096 {
            self.page(false);
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        if self.lace.is_empty() && !self.out.is_empty() {
            let l = self.last;
            self.out[l + 5] |= 4;
            self.out[l + 22..l + 26].fill(0);
            let c = crc(&self.out[l..]);
            self.out[l + 22..l + 26].copy_from_slice(&c.to_le_bytes());
        } else {
            self.page(true);
        }
        self.out
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn pages_roundtrip() {
        assert_eq!(crc(b"123456789"), 0x89a1_897f);
        let ps: Vec<Vec<u8>> = vec![b"head".to_vec(), vec![7u8; 255], vec![], (0..70000u32).map(|i| i as u8).collect(), vec![1, 2, 3]];
        let mut w = Writer::new(0x1234);
        w.packet(&ps[0], 0, true);
        for (i, p) in ps[1..].iter().enumerate() {
            w.packet(p, 100 + i as i64, false);
        }
        let mut f = b"garbage".to_vec();
        f.extend(w.finish());
        let got = packets(&f).unwrap();
        assert_eq!(got.iter().map(|p| p.data.clone()).collect::<Vec<_>>(), ps);
        assert_eq!(got[0].granule, 0);
        assert!(got.last().unwrap().last && got.last().unwrap().granule == 103);
        let mut bad = f.clone();
        let n = bad.len();
        bad[n - 1] ^= 1;
        assert_ne!(packets(&bad).unwrap().len(), ps.len());
        assert!(packets(b"nothing here").is_err());
    }
}
