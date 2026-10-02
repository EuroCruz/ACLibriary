use std::collections::HashMap;

#[derive(Default)]
pub struct Dict {
    m: HashMap<u32, String>,
}

impl Dict {
    pub fn new() -> Self {
        Dict::default()
    }

    pub fn load(text: &str, f: impl Fn(&[u8]) -> u32) -> Self {
        let mut d = Dict::new();
        text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).for_each(|l| {
            d.add(l, &f);
        });
        d
    }

    pub fn add(&mut self, s: &str, f: impl Fn(&[u8]) -> u32) -> u32 {
        let h = f(s.as_bytes());
        self.m.entry(h).or_insert_with(|| s.to_string());
        h
    }

    pub fn name(&self, h: u32) -> Option<&str> {
        self.m.get(&h).map(String::as_str)
    }

    pub fn show(&self, h: u32) -> String {
        self.name(h).map_or_else(|| format!("0x{h:08x}"), str::to_string)
    }

    pub fn len(&self) -> usize {
        self.m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.m.is_empty()
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::hash::FNV1A;

    #[test]
    fn lookup() {
        let d = Dict::load("# names\nfoobar\n\na\n", |s| FNV1A.hash(s));
        assert_eq!(d.len(), 2);
        assert_eq!(d.name(0xBF9C_F968), Some("foobar"));
        assert_eq!(d.show(1), "0x00000001");
    }
}
