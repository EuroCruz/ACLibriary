mod detour;
pub mod lde;
mod slots;

pub use detour::{detour, Hook};
pub use slots::{hook_iat, hook_slot, hook_table, iat_slot, slot, vtable};

#[cfg(test)]
mod tests_support {
    use crate::mem::{alloc_exec, put};

    pub fn code_page(code: &[u8]) -> usize {
        let p = alloc_exec(0x1000).unwrap();
        assert!(put(p, code));
        p
    }
}
