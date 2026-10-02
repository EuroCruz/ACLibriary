use crate::{bad, Res};
use std::path::{Component, Path, PathBuf};

pub fn walk(dir: &Path) -> Res<Vec<PathBuf>> {
    let mut o = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                o.push(p);
            }
        }
    }
    o.sort();
    Ok(o)
}

pub fn with_ext(dir: &Path, ext: &str) -> Res<Vec<PathBuf>> {
    Ok(walk(dir)?.into_iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))).collect())
}

pub fn rel(base: &Path, p: &Path) -> String {
    let r = p.strip_prefix(base).unwrap_or(p);
    r.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn join_safe(base: &Path, r: &str) -> Res<PathBuf> {
    let mut p = base.to_path_buf();
    for s in r.split(['/', '\\']).filter(|s| !s.is_empty() && *s != ".") {
        if s == ".." || s.contains(':') || s.contains('\0') {
            return bad("path escapes base");
        }
        p.push(s);
    }
    Ok(p)
}

pub fn clean_name(s: &str) -> String {
    let o: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let o = o.trim_end_matches(['.', ' ']).to_string();
    if o.is_empty() { "_".to_string() } else { o }
}

pub fn ensure_parent(p: &Path) -> Res<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}

pub fn write(p: &Path, d: &[u8]) -> Res<()> {
    ensure_parent(p)?;
    let tmp = p.with_extension(format!("{}.tmp", p.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, d)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

pub fn unique(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let (stem, ext) = (p.file_stem().and_then(|s| s.to_str()).unwrap_or(""), p.extension().and_then(|s| s.to_str()));
    (1..)
        .map(|i| p.with_file_name(match ext {
            Some(e) => format!("{stem}_{i}.{e}"),
            None => format!("{stem}_{i}"),
        }))
        .find(|c| !c.exists())
        .unwrap()
}

pub fn backup(p: &Path) -> Res<Option<PathBuf>> {
    if !p.exists() {
        return Ok(None);
    }
    let b = p.with_file_name(format!("{}.bak", p.file_name().and_then(|s| s.to_str()).unwrap_or("file")));
    if b.exists() {
        return Ok(None);
    }
    std::fs::copy(p, &b)?;
    Ok(Some(b))
}

pub fn read_opt(p: &Path) -> Option<Vec<u8>> {
    std::fs::read(p).ok()
}

#[cfg(test)]
mod t {
    use super::*;

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("acfs_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn tree() {
        let d = tmp("tree");
        write(&d.join("a/b/x.TXT"), b"1").unwrap();
        write(&d.join("a/y.bin"), b"2").unwrap();
        write(&d.join("z.txt"), b"3").unwrap();
        assert_eq!(walk(&d).unwrap().len(), 3);
        assert_eq!(with_ext(&d, "txt").unwrap().len(), 2);
        assert_eq!(rel(&d, &d.join("a").join("y.bin")), "a/y.bin");
        assert!(!d.join("z.txt.tmp").exists());
        let u = unique(&d.join("z.txt"));
        assert_eq!(u.file_name().unwrap(), "z_1.txt");
        assert!(backup(&d.join("z.txt")).unwrap().is_some());
        assert!(backup(&d.join("z.txt")).unwrap().is_none());
        assert!(backup(&d.join("none")).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names() {
        let b = Path::new("out");
        assert!(join_safe(b, "a/b.txt").is_ok());
        assert!(join_safe(b, "../x").is_err());
        assert_eq!(join_safe(b, "/abs").unwrap(), b.join("abs"));
        assert_eq!(join_safe(b, "dir\\sub/./f.bin").unwrap(), b.join("dir").join("sub").join("f.bin"));
        for e in ["C:/x", "c:\\win", "a\\..\\..\\x", "x/../y", "f:ads", "a\0b"] {
            assert!(join_safe(b, e).is_err(), "{e}");
        }
        assert_eq!(clean_name("a:b?c. "), "a_b_c");
        assert_eq!(clean_name("..."), "_");
    }
}
