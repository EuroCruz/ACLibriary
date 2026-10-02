use crate::win::{waveOutClose, waveOutGetNumDevs, waveOutOpen, waveOutPause, waveOutPrepareHeader, waveOutReset, waveOutRestart, waveOutSetVolume, waveOutUnprepareHeader, waveOutWrite, WaveFormat, WaveHdr, HANDLE};
use ac_core::{Error, Res};
use std::collections::VecDeque;
use std::time::Duration;

const HDR: u32 = std::mem::size_of::<WaveHdr>() as u32;

pub fn devices() -> u32 {
    unsafe { waveOutGetNumDevs() }
}

struct Buf {
    h: WaveHdr,
    _d: Vec<i16>,
}

pub struct Out {
    h: HANDLE,
    q: VecDeque<Box<Buf>>,
    pub channels: u16,
    pub rate: u32,
}

unsafe impl Send for Out {}

impl Out {
    pub fn open(channels: u16, rate: u32) -> Res<Out> {
        let align = channels * 2;
        let f = WaveFormat { tag: 1, channels, rate, bytes_per_sec: rate * align as u32, align, bits: 16, extra: 0 };
        let mut h = std::ptr::null_mut();
        match unsafe { waveOutOpen(&mut h, u32::MAX, &f, 0, 0, 0) } {
            0 => Ok(Out { h, q: VecDeque::new(), channels, rate }),
            e => Err(Error::Msg(format!("audio: waveOutOpen failed ({e})"))),
        }
    }

    fn reap(&mut self) {
        while self.q.front().is_some_and(|b| unsafe { std::ptr::read_volatile(&b.h.flags) } & 1 != 0) {
            let mut b = self.q.pop_front().unwrap();
            unsafe { waveOutUnprepareHeader(self.h, &mut b.h, HDR) };
        }
    }

    pub fn push(&mut self, s: &[i16]) -> Res<()> {
        self.reap();
        if s.is_empty() {
            return Ok(());
        }
        let mut d = s.to_vec();
        let h = WaveHdr { data: d.as_mut_ptr().cast(), len: (d.len() * 2) as u32, recorded: 0, user: 0, flags: 0, loops: 0, next: std::ptr::null_mut(), reserved: 0 };
        let mut b = Box::new(Buf { h, _d: d });
        unsafe {
            if waveOutPrepareHeader(self.h, &mut b.h, HDR) != 0 {
                return Err(Error::Bad("audio: prepare failed"));
            }
            if waveOutWrite(self.h, &mut b.h, HDR) != 0 {
                waveOutUnprepareHeader(self.h, &mut b.h, HDR);
                return Err(Error::Bad("audio: write failed"));
            }
        }
        self.q.push_back(b);
        Ok(())
    }

    pub fn queued(&mut self) -> usize {
        self.reap();
        self.q.len()
    }

    pub fn done(&mut self) -> bool {
        self.queued() == 0
    }

    pub fn wait(&mut self) {
        while !self.done() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn stop(&mut self) {
        unsafe { waveOutReset(self.h) };
        self.reap();
    }

    pub fn pause(&self, on: bool) {
        unsafe {
            if on { waveOutPause(self.h) } else { waveOutRestart(self.h) };
        }
    }

    pub fn volume(&self, l: f32, r: f32) {
        let v = |x: f32| (x.clamp(0.0, 1.0) * 65535.0) as u32;
        unsafe { waveOutSetVolume(self.h, v(l) | v(r) << 16) };
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        self.stop();
        unsafe { waveOutClose(self.h) };
    }
}

pub fn play(s: &[i16], channels: u16, rate: u32) -> Res<Out> {
    let mut o = Out::open(channels, rate)?;
    o.push(s)?;
    Ok(o)
}

pub fn mix(dst: &mut [i16], src: &[i16], gain: f32) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = (*d as f32 + s as f32 * gain).clamp(-32768.0, 32767.0) as i16;
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use std::time::Instant;

    fn tone(n: usize) -> Vec<i16> {
        (0..n).map(|i| ((i as f32 * 0.06).sin() * 300.0) as i16).collect()
    }

    #[test]
    fn mixing() {
        let mut a = vec![30000i16, -30000, 100];
        mix(&mut a, &[10000, -10000, 50], 1.0);
        assert_eq!(a, [32767, -32768, 150]);
        mix(&mut a, &[100, 100, 100], 0.5);
        assert_eq!(a[2], 200);
    }

    #[test]
    fn plays() {
        if devices() == 0 {
            assert!(Out::open(1, 22050).is_err());
            return;
        }
        let t = Instant::now();
        let mut o = play(&tone(2205), 1, 22050).unwrap();
        o.push(&tone(2205)).unwrap();
        assert!(o.queued() >= 1);
        o.volume(0.2, 0.2);
        o.wait();
        assert!(o.done() && t.elapsed() >= Duration::from_millis(150), "{:?}", t.elapsed());
        let mut s = Out::open(2, 44100).unwrap();
        s.push(&tone(44100 * 2)).unwrap();
        s.pause(true);
        s.pause(false);
        let t = Instant::now();
        s.stop();
        assert!(s.done() && t.elapsed() < Duration::from_millis(500));
        drop(s);
        let mut x = Out::open(1, 8000).unwrap();
        x.push(&[]).unwrap();
        assert!(x.done());
    }
}
