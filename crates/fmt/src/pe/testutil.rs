fn put(d: &mut Vec<u8>, at: usize, b: &[u8]) {
    if d.len() < at + b.len() {
        d.resize(at + b.len(), 0);
    }
    d[at..at + b.len()].copy_from_slice(b);
}

fn u32le(d: &mut Vec<u8>, at: usize, v: u32) {
    put(d, at, &v.to_le_bytes());
}

pub fn build(secs: &[(&str, u32, Vec<u8>, u32)], dirs: &[(usize, u32, u32)]) -> Vec<u8> {
    let mut d = vec![0u8; 0x400];
    put(&mut d, 0, b"MZ");
    u32le(&mut d, 0x3c, 0x40);
    put(&mut d, 0x40, b"PE\0\0");
    put(&mut d, 0x44, &0x14cu16.to_le_bytes());
    put(&mut d, 0x46, &(secs.len() as u16).to_le_bytes());
    put(&mut d, 0x54, &0xe0u16.to_le_bytes());
    put(&mut d, 0x56, &0x102u16.to_le_bytes());
    let o = 0x58;
    put(&mut d, o, &0x10bu16.to_le_bytes());
    u32le(&mut d, o + 16, 0x1000);
    u32le(&mut d, o + 28, 0x40_0000);
    u32le(&mut d, o + 32, 0x1000);
    u32le(&mut d, o + 36, 0x200);
    u32le(&mut d, o + 60, 0x400);
    u32le(&mut d, o + 92, 16);
    for &(i, rva, size) in dirs {
        u32le(&mut d, o + 96 + i * 8, rva);
        u32le(&mut d, o + 100 + i * 8, size);
    }
    let table = o + 0xe0;
    let mut image = 0x1000u32;
    for (i, (name, va, data, flags)) in secs.iter().enumerate() {
        let h = table + i * 40;
        put(&mut d, h, name.as_bytes());
        let raw = (data.len() as u32).div_ceil(0x200) * 0x200;
        let ptr = d.len() as u32;
        u32le(&mut d, h + 8, data.len() as u32);
        u32le(&mut d, h + 12, *va);
        u32le(&mut d, h + 16, raw);
        u32le(&mut d, h + 20, ptr);
        u32le(&mut d, h + 36, *flags);
        let mut padded = data.clone();
        padded.resize(raw as usize, 0);
        d.extend_from_slice(&padded);
        image = image.max((va + data.len() as u32).div_ceil(0x1000) * 0x1000);
    }
    u32le(&mut d, o + 56, image);
    d
}

pub fn sample() -> Vec<u8> {
    let mut text = vec![0x90, 0xc3];
    text.resize(0x20, 0);
    let mut data = b"hello\0".to_vec();
    data.resize(0x40, 0xaa);
    build(&[(".text", 0x1000, text, 0x6000_0020), (".data", 0x2000, data, 0xc000_0040)], &[])
}

pub fn with_tables() -> Vec<u8> {
    let rva = 0x2000u32;
    let mut s = vec![0u8; 0x200];
    let (imp, ilt, hint, name, exp, exp_funcs, exp_names, exp_ords, exp_n1, exp_n2, exp_dll) =
        (0x00, 0x30, 0x50, 0x60, 0x80, 0xc0, 0xd0, 0xe0, 0xf0, 0xf8, 0x100);
    let r = |o: u32| rva + o;
    u32le(&mut s, imp, r(ilt as u32));
    u32le(&mut s, imp + 12, r(name as u32));
    u32le(&mut s, imp + 16, r(ilt as u32 + 0x10));
    u32le(&mut s, ilt, r(hint as u32));
    u32le(&mut s, ilt + 4, 0x8000_0005);
    u32le(&mut s, ilt + 0x10, r(hint as u32));
    u32le(&mut s, ilt + 0x14, 0x8000_0005);
    put(&mut s, hint + 2, b"ExitProcess\0");
    put(&mut s, name, b"kernel32.dll\0");
    u32le(&mut s, exp + 12, r(exp_dll as u32));
    u32le(&mut s, exp + 16, 1);
    u32le(&mut s, exp + 20, 2);
    u32le(&mut s, exp + 24, 2);
    u32le(&mut s, exp + 28, r(exp_funcs as u32));
    u32le(&mut s, exp + 32, r(exp_names as u32));
    u32le(&mut s, exp + 36, r(exp_ords as u32));
    u32le(&mut s, exp_funcs, 0x1000);
    u32le(&mut s, exp_funcs + 4, 0x1001);
    u32le(&mut s, exp_names, r(exp_n1 as u32));
    u32le(&mut s, exp_names + 4, r(exp_n2 as u32));
    put(&mut s, exp_ords, &[0, 0, 1, 0]);
    put(&mut s, exp_n1, b"Foo\0");
    put(&mut s, exp_n2, b"Bar\0");
    put(&mut s, exp_dll, b"my.dll\0");
    let mut text = vec![0x90, 0xc3];
    text.resize(0x20, 0);
    build(&[(".text", 0x1000, text, 0x6000_0020), (".rdata", 0x2000, s, 0x4000_0040)], &[(0, rva + exp as u32, 0x40), (1, rva, 0x28)])
}
