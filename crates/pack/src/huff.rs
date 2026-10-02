use ac_core::{bad, Res};

pub struct Dec {
    count: [u16; 17],
    sym: Vec<u16>,
}

impl Dec {
    pub fn new(len: &[u8]) -> Res<Dec> {
        if len.iter().any(|&l| l > 16) {
            return bad("code too long");
        }
        let mut count = [0u16; 17];
        len.iter().for_each(|&l| count[l as usize] += 1);
        count[0] = 0;
        let mut left = 1i32;
        for &c in &count[1..] {
            left = (left << 1) - c as i32;
            if left < 0 {
                return bad("over-subscribed code");
            }
        }
        let mut off = [0u16; 17];
        for i in 1..16 {
            off[i + 1] = off[i] + count[i];
        }
        let mut sym = vec![0u16; len.len()];
        for (s, &l) in len.iter().enumerate() {
            if l != 0 {
                sym[off[l as usize] as usize] = s as u16;
                off[l as usize] += 1;
            }
        }
        Ok(Dec { count, sym })
    }

    pub fn decode(&self, mut bit: impl FnMut() -> Res<u32>) -> Res<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..17 {
            code |= bit()? as i32;
            let c = self.count[len] as i32;
            if code - c < first {
                return Ok(self.sym[(index + (code - first)) as usize]);
            }
            index += c;
            first = (first + c) << 1;
            code <<= 1;
        }
        bad("bad huffman code")
    }
}

pub fn lengths(freq: &[u32], max: u8) -> Vec<u8> {
    let mut f: Vec<u32> = freq.to_vec();
    loop {
        let l = build(&f);
        if l.iter().all(|&x| x <= max) {
            return l;
        }
        f.iter_mut().filter(|x| **x > 0).for_each(|x| *x = (*x / 2).max(1));
    }
}

pub fn full(freq: &[u32], max: u8) -> Vec<u8> {
    let mut f = freq.to_vec();
    let mut i = 0;
    while f.iter().filter(|&&x| x > 0).count() < 2.min(f.len()) {
        f[i] = f[i].max(1);
        i += 1;
    }
    lengths(&f, max)
}

fn build(freq: &[u32]) -> Vec<u8> {
    let used: Vec<usize> = (0..freq.len()).filter(|&i| freq[i] > 0).collect();
    let mut out = vec![0u8; freq.len()];
    match used.len() {
        0 => return out,
        1 => {
            out[used[0]] = 1;
            return out;
        }
        _ => {}
    }
    let mut nodes: Vec<(u64, i32, i32)> = used.iter().map(|&i| (freq[i] as u64, -(i as i32) - 1, -1)).collect();
    let mut live: Vec<usize> = (0..nodes.len()).collect();
    while live.len() > 1 {
        live.sort_by(|&a, &b| nodes[b].0.cmp(&nodes[a].0).then(b.cmp(&a)));
        let (a, b) = (live.pop().unwrap(), live.pop().unwrap());
        nodes.push((nodes[a].0 + nodes[b].0, a as i32, b as i32));
        live.push(nodes.len() - 1);
    }
    let mut stack = vec![(live[0], 0u8)];
    while let Some((n, d)) = stack.pop() {
        let (_, l, r) = nodes[n];
        if r < 0 && l < 0 {
            out[(-l - 1) as usize] = d;
        } else {
            stack.push((l as usize, d + 1));
            stack.push((r as usize, d + 1));
        }
    }
    out
}

pub fn canon(len: &[u8]) -> Vec<u16> {
    let mut count = [0u32; 17];
    len.iter().for_each(|&l| count[l as usize] += 1);
    count[0] = 0;
    let mut next = [0u32; 17];
    let mut c = 0u32;
    for i in 1..17 {
        c = (c + count[i - 1]) << 1;
        next[i] = c;
    }
    len.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let v = next[l as usize];
            next[l as usize] += 1;
            v as u16
        })
        .collect()
}

pub fn codes(len: &[u8]) -> Vec<u16> {
    canon(len).iter().zip(len).map(|(&c, &l)| if l == 0 { 0 } else { c.reverse_bits() >> (16 - l) }).collect()
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn limits_and_prefix() {
        let freq: Vec<u32> = (0..40).map(|i| 1u32 << (i % 30)).collect();
        let l = lengths(&freq, 15);
        assert!(l.iter().all(|&x| x <= 15 && x > 0));
        let kraft: f64 = l.iter().map(|&x| 2f64.powi(-(x as i32))).sum();
        assert!(kraft <= 1.0 + 1e-9);
        assert_eq!(lengths(&[0, 5, 0], 15), [0, 1, 0]);
        assert_eq!(codes(&[2, 1, 2]).len(), 3);
        assert_eq!(canon(&[2, 1, 3, 3]), [2, 0, 6, 7]);
        assert_eq!(full(&[0, 9, 0], 16), [1, 1, 0]);
        assert_eq!(full(&[0, 0, 0], 16), [1, 1, 0]);
    }

    #[test]
    fn msb_roundtrip_16() {
        let l = lengths(&(0..20).map(|i| 1u32 << i).collect::<Vec<_>>(), 16);
        assert!(l.iter().all(|&x| (1..=16).contains(&x)));
        let (d, c) = (Dec::new(&l).unwrap(), canon(&l));
        for s in 0..20 {
            let mut k = l[s];
            let got = d.decode(|| {
                k -= 1;
                Ok((c[s] >> k) as u32 & 1)
            });
            assert_eq!(got.unwrap(), s as u16);
        }
    }
}
