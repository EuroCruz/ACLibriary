use std::time::{Duration, Instant};

pub struct Stopwatch {
    t: Instant,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Stopwatch { t: Instant::now() }
    }
}

impl Stopwatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn secs(&self) -> f64 {
        self.t.elapsed().as_secs_f64()
    }

    pub fn millis(&self) -> u128 {
        self.t.elapsed().as_millis()
    }

    pub fn reset(&mut self) -> f64 {
        let s = self.secs();
        self.t = Instant::now();
        s
    }
}

pub struct Clock {
    last: Instant,
    pub dt: f32,
    pub max_dt: f32,
    pub total: f64,
    pub frames: u64,
    win: f32,
    win_frames: u32,
    fps: f32,
}

impl Clock {
    pub fn new(max_dt: f32) -> Self {
        Clock { last: Instant::now(), dt: 0.0, max_dt, total: 0.0, frames: 0, win: 0.0, win_frames: 0, fps: 0.0 }
    }

    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let raw = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.step(raw)
    }

    pub fn step(&mut self, raw: f32) -> f32 {
        self.dt = raw.min(self.max_dt);
        self.total += self.dt as f64;
        self.frames += 1;
        self.win += raw;
        self.win_frames += 1;
        if self.win >= 0.5 {
            self.fps = self.win_frames as f32 / self.win;
            self.win = 0.0;
            self.win_frames = 0;
        }
        self.dt
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }
}

pub struct Fixed {
    pub step: f32,
    acc: f32,
    pub max_steps: u32,
}

impl Fixed {
    pub fn new(hz: f32) -> Self {
        Fixed { step: 1.0 / hz, acc: 0.0, max_steps: 8 }
    }

    pub fn feed(&mut self, dt: f32) -> u32 {
        self.acc += dt;
        let n = ((self.acc / self.step) as u32).min(self.max_steps);
        self.acc = if n == self.max_steps { 0.0 } else { self.acc - n as f32 * self.step };
        n
    }

    pub fn alpha(&self) -> f32 {
        self.acc / self.step
    }
}

pub struct Limiter {
    frame: Duration,
    next: Instant,
}

impl Limiter {
    pub fn new(fps: f32) -> Self {
        Limiter { frame: if fps > 0.0 { Duration::from_secs_f32(1.0 / fps) } else { Duration::ZERO }, next: Instant::now() }
    }

    pub fn set(&mut self, fps: f32) {
        self.frame = if fps > 0.0 { Duration::from_secs_f32(1.0 / fps) } else { Duration::ZERO };
    }

    pub fn wait(&mut self) {
        if self.frame.is_zero() {
            return;
        }
        self.next += self.frame;
        let now = Instant::now();
        if self.next <= now {
            self.next = now;
            return;
        }
        let coarse = self.next - now;
        if coarse > Duration::from_millis(2) {
            std::thread::sleep(coarse - Duration::from_millis(2));
        }
        while Instant::now() < self.next {
            std::hint::spin_loop();
        }
    }
}

pub fn hms(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

pub fn ms(millis: u64) -> String {
    format!("{}:{:02}.{:03}", millis / 60000, millis / 1000 % 60, millis % 1000)
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn clock() {
        let mut c = Clock::new(0.1);
        assert_eq!(c.step(1.0), 0.1);
        for _ in 0..30 {
            c.step(1.0 / 60.0);
        }
        assert!((c.fps() - 60.0).abs() < 1.0);
        assert_eq!(c.frames, 31);
    }

    #[test]
    fn fixed() {
        let mut f = Fixed::new(100.0);
        assert_eq!(f.feed(0.025), 2);
        assert!((f.alpha() - 0.5).abs() < 1e-3);
        assert_eq!(f.feed(10.0), 8);
        assert_eq!(f.alpha(), 0.0);
    }

    #[test]
    fn limiter() {
        let mut l = Limiter::new(200.0);
        let s = Stopwatch::new();
        for _ in 0..10 {
            l.wait();
        }
        assert!(s.secs() >= 0.04);
        Limiter::new(0.0).wait();
    }

    #[test]
    fn format() {
        assert_eq!(hms(3725), "1:02:05");
        assert_eq!(hms(65), "1:05");
        assert_eq!(ms(61_005), "1:01.005");
    }
}
