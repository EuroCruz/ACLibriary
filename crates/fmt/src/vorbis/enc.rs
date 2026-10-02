use super::bits::{ilog, Bw};
use super::book::{huffman, write_header, Book};
use super::dec::{neighbors, point, Floor1, RANGE};
use super::mdct::{window, Mdct};
use crate::ogg::Writer;

const N: usize = 2048;
const H: usize = N / 2;
const R: i32 = 15;
const SIDE: usize = (2 * R + 1) as usize;
const BIG: i32 = 127;
const SIDE2: usize = (2 * BIG + 1) as usize;
const PS: usize = 16;
const CW: usize = 4;
const MULT: u32 = 2;
const PTS: [u32; 32] = [2, 3, 4, 5, 6, 8, 10, 12, 15, 18, 22, 26, 31, 37, 44, 52, 62, 74, 88, 104, 124, 148, 176, 208, 248, 296, 352, 420, 500, 596, 710, 850];

fn floor() -> Floor1 {
    let mut xs = vec![0, H as u32];
    xs.extend(PTS);
    Floor1 { parts: vec![0; PTS.len() / 4], cdim: vec![4], csub: vec![0], cmaster: vec![0], sbooks: vec![vec![0]], mult: MULT, xs }
}

fn decode_val(val: i32, pred: i32, range: i32) -> i32 {
    let (hroom, lroom) = (range - pred, pred);
    let room = if hroom < lroom { hroom } else { lroom } * 2;
    if val == 0 {
        pred
    } else if val >= room {
        if hroom > lroom { val - lroom + pred } else { pred - val + hroom - 1 }
    } else if val & 1 == 1 {
        pred - (val + 1) / 2
    } else {
        pred + val / 2
    }
}

fn code_floor(f: &Floor1, want: &[i32]) -> Vec<i32> {
    let range = RANGE[MULT as usize - 1];
    let n = f.xs.len();
    let (mut ys, mut fy) = (vec![0i32; n], vec![0i32; n]);
    ys[0] = want[0];
    ys[1] = want[1];
    fy[0] = want[0];
    fy[1] = want[1];
    for i in 2..n {
        let (lo, hi) = neighbors(&f.xs, i);
        let pred = point(f.xs[lo] as i32, fy[lo], f.xs[hi] as i32, fy[hi], f.xs[i] as i32);
        let best = if pred >= want[i] && pred - want[i] <= 1 { 0 } else { (0..range).filter(|&v| decode_val(v, pred, range) >= want[i]).min_by_key(|&v| (decode_val(v, pred, range) - want[i], v)).unwrap_or(0) };
        ys[i] = best;
        fy[i] = decode_val(best, pred, range);
    }
    ys
}

struct Chan {
    ys: Vec<i32>,
    cls: Vec<u8>,
    q: Vec<i32>,
}

fn analyse(f: &Floor1, spec: &[f32], step: f32) -> Option<Chan> {
    let peak = spec.iter().fold(0f32, |m, &x| m.max(x.abs()));
    if peak < 1e-6 {
        return None;
    }
    let mut ord: Vec<usize> = (0..f.xs.len()).collect();
    ord.sort_by_key(|&i| f.xs[i]);
    let l = 1.0649863e-07f64.ln();
    let mut want = vec![0i32; f.xs.len()];
    for (k, &i) in ord.iter().enumerate() {
        let lo = if k == 0 { 0 } else { f.xs[ord[k - 1]] as usize };
        let hi = if k + 1 == ord.len() { H } else { f.xs[ord[k + 1]] as usize + 1 };
        let env = spec[lo.min(H - 1)..hi.clamp(lo.min(H - 1) + 1, H)].iter().fold(0f32, |m, &v| m.max(v.abs())).max(peak * 1e-4);
        let fl = (env * step).max(1.1e-7) as f64;
        let idx = 255.0 - 255.0 * fl.ln() / l;
        want[i] = ((idx / MULT as f64).ceil() as i32).clamp(0, RANGE[MULT as usize - 1] - 1);
    }
    let ys = code_floor(f, &want);
    let (fy, ok) = f.unpack(&ys);
    let curve = f.render(&fy, &ok, H);
    let q: Vec<i32> = spec.iter().zip(&curve).map(|(&x, &c)| ((x / c).round() as i32).clamp(-BIG, BIG)).collect();
    let cls = q.chunks(PS).map(|p| p.iter().map(|v| v.abs()).max().map_or(0, |m| if m == 0 { 0 } else if m <= R { 1 } else { 2 })).collect();
    Some(Chan { ys, cls, q })
}

pub fn encode(channels: usize, rate: u32, pcm: &[f32], quality: f32, comments: &[String]) -> Vec<u8> {
    let f = floor();
    let step = 0.3 * 10f32.powf(-1.5 * quality.clamp(0.0, 1.0));
    let len = pcm.len() / channels.max(1);
    let blocks = len.div_ceil(H) + 1;
    let mut mdct = Mdct::new(N);
    let mut w = vec![0f32; N];
    window(N, H, H, &mut w);
    let mut frames: Vec<Vec<Option<Chan>>> = Vec::with_capacity(blocks);
    let (mut c_floor, mut c_cls, mut c_vq, mut c_big) = (vec![0u64; 128], vec![0u64; 81], vec![0u64; SIDE * SIDE], vec![0u64; SIDE2]);
    let mut x = vec![0f32; N];
    let mut spec = vec![0f32; H];
    for j in 0..blocks {
        let t0 = (j * H) as isize - H as isize;
        let mut fr = Vec::with_capacity(channels);
        for c in 0..channels {
            for (i, v) in x.iter_mut().enumerate() {
                let t = t0 + i as isize;
                *v = if t >= 0 && (t as usize) < len { pcm[t as usize * channels + c] * w[i] } else { 0.0 };
            }
            mdct.forward(&x, &mut spec);
            spec.iter_mut().for_each(|s| *s *= 4.0 / N as f32);
            let ch = analyse(&f, &spec, step);
            if let Some(ch) = &ch {
                ch.ys[2..].iter().for_each(|&y| c_floor[y as usize] += 1);
                for p in ch.cls.chunks(CW) {
                    c_cls[p.iter().fold(0, |a, &b| a * 3 + b as usize)] += 1;
                }
                for (p, &cl) in ch.q.chunks(PS).zip(&ch.cls) {
                    match cl {
                        1 => p.chunks(2).for_each(|v| c_vq[(v[0] + R) as usize + (v[1] + R) as usize * SIDE] += 1),
                        2 => p.iter().for_each(|&v| c_big[(v + BIG) as usize] += 1),
                        _ => {}
                    }
                }
            }
            fr.push(ch);
        }
        frames.push(fr);
    }
    let (l_floor, l_cls, l_vq, l_big) = (huffman(&c_floor, 24), huffman(&c_cls, 24), huffman(&c_vq, 24), huffman(&c_big, 24));
    let bf = Book::new(1, l_floor.clone(), Vec::new()).unwrap();
    let bc = Book::new(CW, l_cls.clone(), Vec::new()).unwrap();
    let bv = Book::new(2, l_vq.clone(), Vec::new()).unwrap();
    let bb = Book::new(1, l_big.clone(), Vec::new()).unwrap();
    let mut og = Writer::new(0x4143_4c42);
    let mut id = Bw::new();
    id.put(1, 8);
    b"vorbis".iter().for_each(|&b| id.put(b as u32, 8));
    id.put(0, 32);
    id.put(channels as u32, 8);
    id.put(rate, 32);
    id.put(0, 32);
    id.put(0, 32);
    id.put(0, 32);
    id.put(8, 4);
    id.put(11, 4);
    id.flag(true);
    og.packet(&id.d, 0, true);
    let mut cm = Bw::new();
    cm.put(3, 8);
    b"vorbis".iter().for_each(|&b| cm.put(b as u32, 8));
    let vendor = b"ACLibriary";
    cm.put(vendor.len() as u32, 32);
    vendor.iter().for_each(|&b| cm.put(b as u32, 8));
    cm.put(comments.len() as u32, 32);
    for c in comments {
        cm.put(c.len() as u32, 32);
        c.bytes().for_each(|b| cm.put(b as u32, 8));
    }
    cm.flag(true);
    og.packet(&cm.d, 0, false);
    let mut st = Bw::new();
    st.put(5, 8);
    b"vorbis".iter().for_each(|&b| st.put(b as u32, 8));
    st.put(3, 8);
    write_header(&mut st, 1, &l_floor, None);
    write_header(&mut st, CW, &l_cls, None);
    write_header(&mut st, 2, &l_vq, Some((-(R as f32), 1.0, ilog(2 * R as u32), SIDE as u32)));
    write_header(&mut st, 1, &l_big, Some((-(BIG as f32), 1.0, ilog(2 * BIG as u32), SIDE2 as u32)));
    st.put(0, 6);
    st.put(0, 16);
    st.put(0, 6);
    st.put(1, 16);
    st.put(f.parts.len() as u32, 5);
    f.parts.iter().for_each(|&p| st.put(p as u32, 4));
    st.put(3, 3);
    st.put(0, 2);
    st.put(1, 8);
    st.put(MULT - 1, 2);
    st.put(10, 4);
    PTS.iter().for_each(|&x| st.put(x, 10));
    st.put(0, 6);
    st.put(1, 16);
    st.put(0, 24);
    st.put(H as u32, 24);
    st.put(PS as u32 - 1, 24);
    st.put(2, 6);
    st.put(1, 8);
    st.put(0, 3);
    st.flag(false);
    st.put(1, 3);
    st.flag(false);
    st.put(1, 3);
    st.flag(false);
    st.put(2, 8);
    st.put(3, 8);
    st.put(0, 6);
    st.put(0, 16);
    st.flag(false);
    st.flag(false);
    st.put(0, 2);
    st.put(0, 8);
    st.put(0, 8);
    st.put(0, 8);
    st.put(0, 6);
    st.flag(true);
    st.put(0, 16);
    st.put(0, 16);
    st.put(0, 8);
    st.flag(true);
    og.packet(&st.d, 0, true);
    let bits = ilog(RANGE[MULT as usize - 1] as u32 - 1);
    for (j, fr) in frames.iter().enumerate() {
        let mut p = Bw::new();
        p.put(0, 1);
        p.flag(true);
        p.flag(true);
        for ch in fr {
            p.flag(ch.is_some());
            if let Some(ch) = ch {
                p.put(ch.ys[0] as u32, bits);
                p.put(ch.ys[1] as u32, bits);
                ch.ys[2..].iter().for_each(|&y| bf.write(&mut p, y as usize));
            }
        }
        let parts = H / PS;
        for pc in (0..parts).step_by(CW) {
            for ch in fr.iter().flatten() {
                bc.write(&mut p, ch.cls[pc..pc + CW].iter().fold(0, |a, &b| a * 3 + b as usize));
            }
            for k in pc..pc + CW {
                for ch in fr.iter().flatten() {
                    let part = &ch.q[k * PS..(k + 1) * PS];
                    match ch.cls[k] {
                        1 => part.chunks(2).for_each(|v| bv.write(&mut p, (v[0] + R) as usize + (v[1] + R) as usize * SIDE)),
                        2 => part.iter().for_each(|&v| bb.write(&mut p, (v + BIG) as usize)),
                        _ => {}
                    }
                }
            }
        }
        og.packet(&p.d, ((j * H) as i64).min(len as i64), false);
    }
    og.finish()
}
