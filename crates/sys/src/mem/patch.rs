use crate::mem::{branch, get, put};
use ac_core::{bad, Res};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Original,
    Patched,
    Different,
}

#[derive(Clone, Debug)]
pub struct Patch {
    pub name: String,
    pub addr: usize,
    pub expect: Vec<u8>,
    pub with: Vec<u8>,
}

impl Patch {
    pub fn new(name: &str, addr: usize, expect: &[u8], with: &[u8]) -> Patch {
        assert_eq!(expect.len(), with.len(), "patch {name}: length mismatch");
        Patch { name: name.to_string(), addr, expect: expect.to_vec(), with: with.to_vec() }
    }

    fn jump_to(op: u8, name: &str, addr: usize, expect: &[u8], to: usize, tail: &[u8]) -> Patch {
        let mut w = branch(op, addr, to).to_vec();
        w.extend_from_slice(tail);
        Patch::new(name, addr, expect, &w)
    }

    pub fn call(name: &str, addr: usize, expect: &[u8], to: usize, tail: &[u8]) -> Patch {
        Patch::jump_to(0xE8, name, addr, expect, to, tail)
    }

    pub fn jump(name: &str, addr: usize, expect: &[u8], to: usize, tail: &[u8]) -> Patch {
        Patch::jump_to(0xE9, name, addr, expect, to, tail)
    }

    pub fn state(&self) -> Option<State> {
        let cur = get(self.addr, self.expect.len())?;
        Some(if cur == self.expect {
            State::Original
        } else if cur == self.with {
            State::Patched
        } else {
            State::Different
        })
    }

    pub fn apply(&self) -> Res<State> {
        match self.state() {
            None => bad("memory not readable"),
            Some(State::Original) => put(self.addr, &self.with).then_some(State::Original).ok_or(ac_core::Error::Bad("memory not writable")),
            Some(s) => Ok(s),
        }
    }

    pub fn revert(&self) -> Res<State> {
        match self.state() {
            None => bad("memory not readable"),
            Some(State::Patched) => put(self.addr, &self.expect).then_some(State::Patched).ok_or(ac_core::Error::Bad("memory not writable")),
            Some(s) => Ok(s),
        }
    }
}

#[derive(Default)]
pub struct Group(pub Vec<Patch>);

impl Group {
    pub fn add(&mut self, p: Patch) -> &mut Self {
        self.0.push(p);
        self
    }

    pub fn apply(&self) -> Vec<(String, Res<State>)> {
        self.0.iter().map(|p| (p.name.clone(), p.apply())).collect()
    }

    pub fn revert(&self) -> Vec<(String, Res<State>)> {
        self.0.iter().rev().map(|p| (p.name.clone(), p.revert())).collect()
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::mem::{alloc_exec, free_exec, put};

    #[test]
    fn states_and_undo() {
        let p = alloc_exec(0x1000).unwrap();
        put(p, &[1, 2, 3]);
        let patch = Patch::new("t", p, &[1, 2, 3], &[9, 9, 9]);
        assert_eq!(patch.state(), Some(State::Original));
        assert_eq!(patch.apply().unwrap(), State::Original);
        assert_eq!(patch.state(), Some(State::Patched));
        assert_eq!(patch.apply().unwrap(), State::Patched);
        assert_eq!(patch.revert().unwrap(), State::Patched);
        assert_eq!(patch.state(), Some(State::Original));
        put(p, &[7, 7, 7]);
        assert_eq!(patch.apply().unwrap(), State::Different);
        assert_eq!(get(p, 3), Some(vec![7, 7, 7]));
        assert!(Patch::new("x", 0, &[1], &[2]).apply().is_err());
        free_exec(p);
    }

    #[test]
    fn call_patch_is_relative_to_next_instruction() {
        let c = Patch::call("c", 0x6fabe9, &[0; 7], 0x706130, &[0x85, 0xc0]);
        assert_eq!(c.with, [0xe8, 0x42, 0xb5, 0x00, 0x00, 0x85, 0xc0]);
        assert_eq!(Patch::jump("j", 0x1000, &[0; 5], 0x0ff0, &[]).with, [0xe9, 0xeb, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn groups() {
        let p = alloc_exec(0x1000).unwrap();
        put(p, &[1, 2]);
        let mut g = Group::default();
        g.add(Patch::new("a", p, &[1], &[5])).add(Patch::new("b", p + 1, &[2], &[6]));
        assert!(g.apply().iter().all(|(_, r)| r.is_ok()));
        assert_eq!(get(p, 2), Some(vec![5, 6]));
        g.revert();
        assert_eq!(get(p, 2), Some(vec![1, 2]));
        free_exec(p);
    }
}
