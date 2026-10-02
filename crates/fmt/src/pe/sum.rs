use crate::pe::Pe;

pub fn checksum(d: &[u8], field: usize) -> u32 {
    let mut sum = 0u64;
    for (i, c) in d.chunks(4).enumerate() {
        if i * 4 == field {
            continue;
        }
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        sum += u32::from_le_bytes(b) as u64;
        sum = (sum & 0xffff_ffff) + (sum >> 32);
    }
    sum = (sum & 0xffff) + (sum >> 16);
    sum += sum >> 16;
    (sum & 0xffff) as u32 + d.len() as u32
}

impl Pe {
    pub fn calc_checksum(&self) -> u32 {
        checksum(&self.d, self.opt + 64)
    }

    pub fn fix_checksum(&mut self) -> u32 {
        let c = self.calc_checksum();
        self.set_checksum(c);
        c
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::pe::testutil::sample;

    #[test]
    fn independent_of_stored_value() {
        let mut p = Pe::parse(sample()).unwrap();
        let a = p.calc_checksum();
        p.set_checksum(0xdead_beef);
        assert_eq!(p.calc_checksum(), a);
        assert_eq!(p.fix_checksum(), a);
        assert_eq!(p.checksum(), a);
        assert_eq!(checksum(&[1, 0, 0, 0], 99), 1 + 4);
        assert_eq!(checksum(&[0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0], 99), 1 + 8);
    }
}
