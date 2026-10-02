use super::Opts;

pub struct Out<'o> {
    pub o: &'o Opts,
    pub f32: bool,
    s: String,
    ind: i32,
    pos: usize,
    li: usize,
    flat: bool,
}

impl<'o> Out<'o> {
    pub fn new(o: &'o Opts, f32: bool) -> Out<'o> {
        Out { o, f32, s: String::new(), ind: 0, pos: 0, li: 0, flat: false }
    }

    pub fn flat(&self) -> Out<'o> {
        Out { flat: true, ..Out::new(self.o, self.f32) }
    }

    pub fn wraps(&self) -> bool {
        !self.flat && self.o.width > 0
    }

    pub fn lim(&self) -> usize {
        if self.o.width == 0 { usize::MAX } else { self.o.width }
    }

    pub fn col(&self) -> usize {
        if self.pos == 0 {
            self.ind.max(0) as usize * self.o.tab_w
        } else {
            self.li * self.o.tab_w + self.pos - self.li * self.o.tab.len()
        }
    }

    pub fn done(self) -> String {
        self.s
    }

    pub fn indent(&mut self) {
        self.ind += 1;
    }

    pub fn dedent(&mut self) {
        self.ind -= 1;
    }

    pub fn level(&self) -> i32 {
        self.ind
    }

    pub fn set_level(&mut self, l: i32) {
        self.ind = l;
    }

    pub fn p(&mut self, s: &str) {
        if self.pos == 0 {
            self.li = self.ind.max(0) as usize;
            for _ in 0..self.li {
                self.s.push_str(&self.o.tab);
                self.pos += self.o.tab.len();
            }
        }
        self.s.push_str(s);
        self.pos += s.len();
    }

    pub fn eq(&mut self) {
        let o = self.o;
        self.p(&o.eq);
    }

    pub fn nl(&mut self) {
        self.s.push_str(&self.o.eol);
        self.pos = 0;
    }

    pub fn line(&mut self, s: &str) {
        self.p(s);
        self.nl();
    }
}
