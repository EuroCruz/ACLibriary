use ac_core::hash::adler32;

const WSIZE: usize = 1 << 15;
const WMASK: usize = WSIZE - 1;
const HBITS: u32 = 15;
const HMASK: u32 = (1 << HBITS) - 1;
const HSHIFT: u32 = (HBITS + 2) / 3;
const MINM: usize = 3;
const MAXM: usize = 258;
const LOOK: usize = MAXM + MINM + 1;
const MAXD: usize = WSIZE - LOOK;
const LITBUF: usize = 1 << 14;
const LCODES: usize = 286;
const DCODES: usize = 30;
const BLCODES: usize = 19;
const HEAP: usize = 2 * LCODES + 1;
const EOB: usize = 256;

const LEXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DEXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const BLEXTRA: [u8; 19] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 7];
const BLORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

const CFG: [(usize, usize, usize, usize, bool); 10] = [
    (0, 0, 0, 0, false),
    (4, 4, 8, 4, false),
    (4, 5, 16, 8, false),
    (4, 6, 32, 32, false),
    (4, 4, 16, 16, true),
    (8, 16, 32, 32, true),
    (8, 16, 128, 128, true),
    (8, 32, 128, 256, true),
    (32, 128, 258, 1024, true),
    (32, 258, 258, 4096, true),
];

struct Tab {
    lcode: [u8; 256],
    lbase: [u16; 29],
    dcode: [u8; 512],
    dbase: [u16; 30],
    sl: Vec<u8>,
    sc: Vec<u16>,
}

fn rev(c: u32, n: u32) -> u16 {
    (c.reverse_bits() >> (32 - n)) as u16
}

fn tables() -> Tab {
    let mut t = Tab { lcode: [0; 256], lbase: [0; 29], dcode: [0; 512], dbase: [0; 30], sl: vec![0; 288], sc: vec![0; 288] };
    let mut len = 0usize;
    for c in 0..28 {
        t.lbase[c] = len as u16;
        for _ in 0..1 << LEXTRA[c] {
            t.lcode[len] = c as u8;
            len += 1;
        }
    }
    t.lcode[len - 1] = 28;
    let mut d = 0usize;
    for c in 0..16 {
        t.dbase[c] = d as u16;
        for _ in 0..1 << DEXTRA[c] {
            t.dcode[d] = c as u8;
            d += 1;
        }
    }
    d >>= 7;
    for c in 16..30 {
        t.dbase[c] = (d << 7) as u16;
        for _ in 0..1 << (DEXTRA[c] - 7) {
            t.dcode[256 + d] = c as u8;
            d += 1;
        }
    }
    for n in 0..288 {
        t.sl[n] = match n {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let mut cnt = [0u16; 16];
    t.sl.iter().for_each(|&l| cnt[l as usize] += 1);
    let mut next = [0u16; 16];
    let mut code = 0u16;
    for b in 1..16 {
        code = (code + cnt[b - 1]) << 1;
        next[b] = code;
    }
    for n in 0..288 {
        let l = t.sl[n] as usize;
        t.sc[n] = rev(next[l] as u32, l as u32);
        next[l] += 1;
    }
    t
}

impl Tab {
    fn d(&self, dist: usize) -> usize {
        if dist < 256 {
            self.dcode[dist] as usize
        } else {
            self.dcode[256 + (dist >> 7)] as usize
        }
    }
}

#[derive(Clone)]
struct Tree {
    freq: Vec<u32>,
    dad: Vec<u16>,
    len: Vec<u16>,
    code: Vec<u16>,
    max: isize,
}

impl Tree {
    fn new() -> Tree {
        Tree { freq: vec![0; HEAP], dad: vec![0; HEAP], len: vec![0; HEAP + 1], code: vec![0; HEAP], max: 0 }
    }
}

struct Bits {
    o: Vec<u8>,
    b: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, n: u32) {
        self.b |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.o.push(self.b as u8);
            self.b >>= 8;
            self.n -= 8;
        }
    }

    fn align(&mut self) {
        if self.n > 0 {
            self.o.push(self.b as u8);
        }
        self.b = 0;
        self.n = 0;
    }
}

struct Z<'a> {
    src: &'a [u8],
    at: usize,
    win: Vec<u8>,
    head: Vec<u16>,
    prev: Vec<u16>,
    h: u32,
    ss: usize,
    la: usize,
    bs: isize,
    ms: usize,
    ml: usize,
    pl: usize,
    pm: usize,
    avail: bool,
    good: usize,
    lazy: usize,
    nice: usize,
    chain: usize,
    syms: Vec<(u16, u8)>,
    lt: Tree,
    dt: Tree,
    bt: Tree,
    depth: Vec<u8>,
    heap: Vec<usize>,
    hlen: usize,
    hmax: usize,
    blc: [u16; 16],
    opt: i64,
    stat: i64,
    t: Tab,
    out: Bits,
}

impl Z<'_> {
    fn upd(&mut self, c: u8) {
        self.h = ((self.h << HSHIFT) ^ c as u32) & HMASK;
    }

    fn insert(&mut self, s: usize) -> usize {
        let c = self.win[s + MINM - 1];
        self.upd(c);
        let m = self.head[self.h as usize];
        self.prev[s & WMASK] = m;
        self.head[self.h as usize] = s as u16;
        m as usize
    }

    fn fill(&mut self) {
        loop {
            let mut more = 2 * WSIZE - self.la - self.ss;
            if self.ss >= WSIZE + MAXD {
                self.win.copy_within(WSIZE..2 * WSIZE, 0);
                self.ms = self.ms.wrapping_sub(WSIZE);
                self.ss -= WSIZE;
                self.bs -= WSIZE as isize;
                for p in self.head.iter_mut().chain(self.prev.iter_mut()) {
                    *p = if *p as usize >= WSIZE { *p - WSIZE as u16 } else { 0 };
                }
                more += WSIZE;
            }
            if self.at >= self.src.len() {
                return;
            }
            let n = more.min(self.src.len() - self.at);
            let to = self.ss + self.la;
            self.win[to..to + n].copy_from_slice(&self.src[self.at..self.at + n]);
            self.at += n;
            self.la += n;
            if self.la >= MINM {
                self.h = self.win[self.ss] as u32;
                let c = self.win[self.ss + 1];
                self.upd(c);
            }
            if self.la >= LOOK || self.at >= self.src.len() {
                return;
            }
        }
    }

    fn longest(&mut self, mut cur: usize) -> usize {
        let mut chain = self.chain;
        let s = self.ss;
        let mut best = self.pl;
        let mut nice = self.nice;
        let limit = if s > MAXD { s - MAXD } else { 0 };
        if self.pl >= self.good {
            chain >>= 2;
        }
        if nice > self.la {
            nice = self.la;
        }
        loop {
            let m = cur;
            if self.win[m + best] == self.win[s + best] && self.win[m + best - 1] == self.win[s + best - 1] && self.win[m] == self.win[s] && self.win[m + 1] == self.win[s + 1] {
                let mut l = 3;
                while l < MAXM && self.win[s + l] == self.win[m + l] {
                    l += 1;
                }
                if l > best {
                    self.ms = cur;
                    best = l;
                    if l >= nice {
                        break;
                    }
                }
            }
            cur = self.prev[cur & WMASK] as usize;
            if cur <= limit {
                break;
            }
            chain -= 1;
            if chain == 0 {
                break;
            }
        }
        best.min(self.la)
    }

    fn lit(&mut self, c: u8) -> bool {
        self.syms.push((0, c));
        self.lt.freq[c as usize] += 1;
        self.syms.len() == LITBUF - 1
    }

    fn dist(&mut self, d: usize, l: usize) -> bool {
        self.syms.push((d as u16, l as u8));
        self.lt.freq[self.t.lcode[l] as usize + 257] += 1;
        let c = self.t.d(d - 1);
        self.dt.freq[c] += 1;
        self.syms.len() == LITBUF - 1
    }

    fn smaller(&self, tr: &Tree, n: usize, m: usize) -> bool {
        tr.freq[n] < tr.freq[m] || (tr.freq[n] == tr.freq[m] && self.depth[n] <= self.depth[m])
    }

    fn down(&mut self, tr: &Tree, mut k: usize) {
        let v = self.heap[k];
        let mut j = k << 1;
        while j <= self.hlen {
            if j < self.hlen && self.smaller(tr, self.heap[j + 1], self.heap[j]) {
                j += 1;
            }
            if self.smaller(tr, v, self.heap[j]) {
                break;
            }
            self.heap[k] = self.heap[j];
            k = j;
            j <<= 1;
        }
        self.heap[k] = v;
    }

    fn build(&mut self, tr: &mut Tree, elems: usize, stat: Option<&[u8]>, extra: &[u8], base: usize, maxlen: u16) {
        let mut max: isize = -1;
        self.hlen = 0;
        self.hmax = HEAP;
        for n in 0..elems {
            if tr.freq[n] != 0 {
                self.hlen += 1;
                self.heap[self.hlen] = n;
                max = n as isize;
                self.depth[n] = 0;
            } else {
                tr.len[n] = 0;
            }
        }
        while self.hlen < 2 {
            let node = if max < 2 {
                max += 1;
                max as usize
            } else {
                0
            };
            self.hlen += 1;
            self.heap[self.hlen] = node;
            tr.freq[node] = 1;
            self.depth[node] = 0;
            self.opt -= 1;
            if let Some(s) = stat {
                self.stat -= s[node] as i64;
            }
        }
        tr.max = max;
        for n in (1..=self.hlen / 2).rev() {
            self.down(tr, n);
        }
        let mut node = elems;
        loop {
            let n = self.heap[1];
            self.heap[1] = self.heap[self.hlen];
            self.hlen -= 1;
            self.down(tr, 1);
            let m = self.heap[1];
            self.hmax -= 1;
            self.heap[self.hmax] = n;
            self.hmax -= 1;
            self.heap[self.hmax] = m;
            tr.freq[node] = tr.freq[n] + tr.freq[m];
            self.depth[node] = self.depth[n].max(self.depth[m]) + 1;
            tr.dad[n] = node as u16;
            tr.dad[m] = node as u16;
            self.heap[1] = node;
            node += 1;
            self.down(tr, 1);
            if self.hlen < 2 {
                break;
            }
        }
        self.hmax -= 1;
        self.heap[self.hmax] = self.heap[1];
        self.bitlen(tr, stat, extra, base, maxlen);
        let mut next = [0u16; 16];
        let mut code = 0u16;
        for b in 1..16 {
            code = (code + self.blc[b - 1]) << 1;
            next[b] = code;
        }
        for n in 0..=(tr.max.max(-1) as isize) {
            let l = tr.len[n as usize];
            if l != 0 {
                tr.code[n as usize] = rev(next[l as usize] as u32, l as u32);
                next[l as usize] += 1;
            }
        }
    }

    fn bitlen(&mut self, tr: &mut Tree, stat: Option<&[u8]>, extra: &[u8], base: usize, maxlen: u16) {
        self.blc = [0; 16];
        let mut over = 0i32;
        tr.len[self.heap[self.hmax]] = 0;
        let mut h = self.hmax + 1;
        while h < HEAP {
            let n = self.heap[h];
            let mut bits = tr.len[tr.dad[n] as usize] + 1;
            if bits > maxlen {
                bits = maxlen;
                over += 1;
            }
            tr.len[n] = bits;
            h += 1;
            if n as isize > tr.max {
                continue;
            }
            self.blc[bits as usize] += 1;
            let x = if n >= base { extra[n - base] as i64 } else { 0 };
            let f = tr.freq[n] as i64;
            self.opt += f * (bits as i64 + x);
            if let Some(s) = stat {
                self.stat += f * (s[n] as i64 + x);
            }
        }
        if over == 0 {
            return;
        }
        loop {
            let mut bits = maxlen as usize - 1;
            while self.blc[bits] == 0 {
                bits -= 1;
            }
            self.blc[bits] -= 1;
            self.blc[bits + 1] += 2;
            self.blc[maxlen as usize] -= 1;
            over -= 2;
            if over <= 0 {
                break;
            }
        }
        let mut h = HEAP;
        for bits in (1..=maxlen as usize).rev() {
            let mut n = self.blc[bits];
            while n != 0 {
                h -= 1;
                let m = self.heap[h];
                if m as isize > tr.max {
                    continue;
                }
                if tr.len[m] as usize != bits {
                    self.opt += (bits as i64 - tr.len[m] as i64) * tr.freq[m] as i64;
                    tr.len[m] = bits as u16;
                }
                n -= 1;
            }
        }
    }

    fn scan(&mut self, tr: &mut Tree) {
        let max = tr.max;
        let mut prevlen: i32 = -1;
        let mut next = tr.len[0] as i32;
        let mut count = 0;
        let (mut maxc, mut minc) = if next == 0 { (138, 3) } else { (7, 4) };
        tr.len[(max + 1) as usize] = 0xffff;
        for n in 0..=max as usize {
            let cur = next;
            next = tr.len[n + 1] as i32;
            count += 1;
            if count < maxc && cur == next {
                continue;
            } else if count < minc {
                self.bt.freq[cur as usize] += count as u32;
            } else if cur != 0 {
                if cur != prevlen {
                    self.bt.freq[cur as usize] += 1;
                }
                self.bt.freq[16] += 1;
            } else if count <= 10 {
                self.bt.freq[17] += 1;
            } else {
                self.bt.freq[18] += 1;
            }
            count = 0;
            prevlen = cur;
            (maxc, minc) = if next == 0 {
                (138, 3)
            } else if cur == next {
                (6, 3)
            } else {
                (7, 4)
            };
        }
    }

    fn send(&mut self, tr: &Tree) {
        let max = tr.max;
        let mut prevlen: i32 = -1;
        let mut next = tr.len[0] as i32;
        let mut count = 0;
        let (mut maxc, mut minc) = if next == 0 { (138, 3) } else { (7, 4) };
        let bl = |z: &mut Z, c: usize| {
            let (code, len) = (z.bt.code[c], z.bt.len[c]);
            z.out.put(code as u32, len as u32);
        };
        for n in 0..=max as usize {
            let cur = next;
            next = tr.len[n + 1] as i32;
            count += 1;
            if count < maxc && cur == next {
                continue;
            } else if count < minc {
                for _ in 0..count {
                    bl(self, cur as usize);
                }
            } else if cur != 0 {
                if cur != prevlen {
                    bl(self, cur as usize);
                    count -= 1;
                }
                bl(self, 16);
                self.out.put(count as u32 - 3, 2);
            } else if count <= 10 {
                bl(self, 17);
                self.out.put(count as u32 - 3, 3);
            } else {
                bl(self, 18);
                self.out.put(count as u32 - 11, 7);
            }
            count = 0;
            prevlen = cur;
            (maxc, minc) = if next == 0 {
                (138, 3)
            } else if cur == next {
                (6, 3)
            } else {
                (7, 4)
            };
        }
    }

    fn emit(&mut self, lc: &[u16], ll: &[u8], dc: &[u16], dl: &[u8]) {
        let syms = std::mem::take(&mut self.syms);
        for &(d, l) in &syms {
            if d == 0 {
                self.out.put(lc[l as usize] as u32, ll[l as usize] as u32);
            } else {
                let c = self.t.lcode[l as usize] as usize;
                self.out.put(lc[c + 257] as u32, ll[c + 257] as u32);
                if LEXTRA[c] != 0 {
                    self.out.put(l as u32 - self.t.lbase[c] as u32, LEXTRA[c] as u32);
                }
                let dd = d as usize - 1;
                let c = self.t.d(dd);
                self.out.put(dc[c] as u32, dl[c] as u32);
                if DEXTRA[c] != 0 {
                    self.out.put((dd - self.t.dbase[c] as usize) as u32, DEXTRA[c] as u32);
                }
            }
        }
        self.out.put(lc[EOB] as u32, ll[EOB] as u32);
        self.syms = syms;
        self.syms.clear();
    }

    fn flush(&mut self, last: bool) {
        let stored = (self.ss as isize - self.bs) as usize;
        let mut lt = std::mem::replace(&mut self.lt, Tree::new());
        let mut dt = std::mem::replace(&mut self.dt, Tree::new());
        let sl = self.t.sl.clone();
        self.build(&mut lt, LCODES, Some(&sl), &LEXTRA, 257, 15);
        self.build(&mut dt, DCODES, Some(&[5; 30]), &DEXTRA, 0, 15);
        self.scan(&mut lt);
        self.scan(&mut dt);
        let mut bt = std::mem::replace(&mut self.bt, Tree::new());
        self.build(&mut bt, BLCODES, None, &BLEXTRA, 0, 7);
        self.bt = bt;
        let mut maxbl = BLCODES - 1;
        while maxbl >= 3 && self.bt.len[BLORDER[maxbl]] == 0 {
            maxbl -= 1;
        }
        self.opt += 3 * (maxbl as i64 + 1) + 14;
        let mut optb = (self.opt + 3 + 7) >> 3;
        let statb = (self.stat + 3 + 7) >> 3;
        if statb <= optb {
            optb = statb;
        }
        if stored as i64 + 4 <= optb && self.bs >= 0 {
            self.out.put(last as u32, 3);
            self.out.align();
            self.out.o.extend((stored as u16).to_le_bytes());
            self.out.o.extend((!(stored as u16)).to_le_bytes());
            let b = self.bs as usize;
            let w = self.win[b..b + stored].to_vec();
            self.out.o.extend(w);
            self.syms.clear();
        } else if statb == optb {
            self.out.put(2 + last as u32, 3);
            let (sc, sl) = (self.t.sc.clone(), self.t.sl.clone());
            let dc: Vec<u16> = (0..30).map(|n| rev(n, 5)).collect();
            self.emit(&sc, &sl, &dc, &[5; 30]);
        } else {
            self.out.put(4 + last as u32, 3);
            let (lc, dc) = (lt.max as u32 + 1, dt.max as u32 + 1);
            self.out.put(lc - 257, 5);
            self.out.put(dc - 1, 5);
            self.out.put(maxbl as u32 + 1 - 4, 4);
            for &r in &BLORDER[..=maxbl] {
                let l = self.bt.len[r];
                self.out.put(l as u32, 3);
            }
            self.send(&lt);
            self.send(&dt);
            let ll: Vec<u8> = lt.len.iter().map(|&x| x as u8).collect();
            let dl: Vec<u8> = dt.len.iter().map(|&x| x as u8).collect();
            let (lcd, dcd) = (lt.code.clone(), dt.code.clone());
            self.emit(&lcd, &ll, &dcd, &dl);
        }
        self.lt = Tree::new();
        self.dt = Tree::new();
        self.bt = Tree::new();
        self.lt.freq[EOB] = 1;
        self.opt = 0;
        self.stat = 0;
        self.bs = self.ss as isize;
        if last {
            self.out.align();
        }
    }

    fn fast(&mut self) {
        loop {
            if self.la < LOOK {
                self.fill();
                if self.la == 0 {
                    break;
                }
            }
            let mut head = 0;
            if self.la >= MINM {
                head = self.insert(self.ss);
            }
            if head != 0 && self.ss - head <= MAXD {
                self.ml = self.longest(head);
            }
            let f;
            if self.ml >= MINM {
                f = self.dist(self.ss - self.ms, self.ml - MINM);
                self.la -= self.ml;
                if self.ml <= self.lazy && self.la >= MINM {
                    self.ml -= 1;
                    while self.ml > 0 {
                        self.ss += 1;
                        self.insert(self.ss);
                        self.ml -= 1;
                    }
                    self.ss += 1;
                } else {
                    self.ss += self.ml;
                    self.ml = 0;
                    self.h = self.win[self.ss] as u32;
                    let c = self.win[self.ss + 1];
                    self.upd(c);
                }
            } else {
                let c = self.win[self.ss];
                f = self.lit(c);
                self.la -= 1;
                self.ss += 1;
            }
            if f {
                self.flush(false);
            }
        }
        self.flush(true);
    }

    fn slow(&mut self) {
        loop {
            if self.la < LOOK {
                self.fill();
                if self.la == 0 {
                    break;
                }
            }
            let mut head = 0;
            if self.la >= MINM {
                head = self.insert(self.ss);
            }
            self.pl = self.ml;
            self.pm = self.ms;
            self.ml = MINM - 1;
            if head != 0 && self.pl < self.lazy && self.ss - head <= MAXD {
                self.ml = self.longest(head);
                if self.ml == MINM && self.ss - self.ms > 4096 {
                    self.ml = MINM - 1;
                }
            }
            if self.pl >= MINM && self.ml <= self.pl {
                let maxins = self.ss + self.la - MINM;
                let f = self.dist(self.ss - 1 - self.pm, self.pl - MINM);
                self.la -= self.pl - 1;
                self.pl -= 2;
                while self.pl > 0 {
                    self.ss += 1;
                    if self.ss <= maxins {
                        self.insert(self.ss);
                    }
                    self.pl -= 1;
                }
                self.avail = false;
                self.ml = MINM - 1;
                self.ss += 1;
                if f {
                    self.flush(false);
                }
            } else if self.avail {
                let c = self.win[self.ss - 1];
                if self.lit(c) {
                    self.flush(false);
                }
                self.ss += 1;
                self.la -= 1;
            } else {
                self.avail = true;
                self.ss += 1;
                self.la -= 1;
            }
        }
        if self.avail {
            let c = self.win[self.ss - 1];
            self.lit(c);
            self.avail = false;
        }
        self.flush(true);
    }
}

pub fn deflate_zc(d: &[u8], level: u32) -> Vec<u8> {
    let (good, lazy, nice, chain, slow) = CFG[level.clamp(1, 9) as usize];
    let mut z = Z {
        src: d,
        at: 0,
        win: vec![0; 2 * WSIZE + MAXM],
        head: vec![0; 1 << HBITS],
        prev: vec![0; WSIZE],
        h: 0,
        ss: 0,
        la: 0,
        bs: 0,
        ms: 0,
        ml: MINM - 1,
        pl: MINM - 1,
        pm: 0,
        avail: false,
        good,
        lazy,
        nice,
        chain,
        syms: Vec::with_capacity(LITBUF),
        lt: Tree::new(),
        dt: Tree::new(),
        bt: Tree::new(),
        depth: vec![0; HEAP],
        heap: vec![0; HEAP + 1],
        hlen: 0,
        hmax: 0,
        blc: [0; 16],
        opt: 0,
        stat: 0,
        t: tables(),
        out: Bits { o: Vec::with_capacity(d.len() / 2 + 64), b: 0, n: 0 },
    };
    z.lt.freq[EOB] = 1;
    if slow {
        z.slow();
    } else {
        z.fast();
    }
    z.out.o
}

pub fn zlib_zc(d: &[u8], level: u32) -> Vec<u8> {
    let lv = level.clamp(1, 9);
    let flags = match lv {
        1 => 0,
        2..=5 => 1,
        6 => 2,
        _ => 3,
    };
    let h = (0x78u16 << 8) | (flags << 6);
    let h = h + 31 - h % 31;
    let mut o = h.to_be_bytes().to_vec();
    o.extend(deflate_zc(d, lv));
    o.extend(adler32(d).to_be_bytes());
    o
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::flate::unzlib;

    #[test]
    fn roundtrips() {
        let mut d = Vec::new();
        for i in 0..200_000u32 {
            d.push(((i * 7) ^ (i >> 5)) as u8 % 13);
        }
        for lv in [1, 3, 6, 9] {
            let z = zlib_zc(&d, lv);
            assert_eq!(unzlib(&z).unwrap(), d, "level {lv}");
            assert_eq!(unzlib(&zlib_zc(b"", lv)).unwrap(), b"");
            assert_eq!(unzlib(&zlib_zc(b"a", lv)).unwrap(), b"a");
        }
        assert_eq!(zlib_zc(b"", 1), [0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
    }
}

#[cfg(test)]
mod reference {
    use super::*;

    #[test]
    #[ignore]
    fn matches_zlib() {
        let dir = std::env::var("AC_ZC").unwrap_or(r"C:\Users\Andrew\AppData\Local\Temp\zc".into());
        let mut bad = Vec::new();
        for k in ["small", "text", "rand", "low", "zero", "exe"] {
            let d = std::fs::read(format!("{dir}/{k}.in")).unwrap();
            for lv in 1..=9 {
                let want = std::fs::read(format!("{dir}/{k}.{lv}")).unwrap();
                let got = zlib_zc(&d, lv);
                if got != want {
                    let at = got.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
                    bad.push(format!("{k} level {lv}: differs at {at} ({} vs {} bytes)", got.len(), want.len()));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }
}
