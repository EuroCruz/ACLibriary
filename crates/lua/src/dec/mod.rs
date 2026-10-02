mod block;
mod expr;
mod fun;
mod out;
mod reg;
mod stmt;
mod var;

use crate::luac::{self, Chunk, Proto};
use ac_core::Res;

type R<T = ()> = Res<T>;

#[derive(Clone, Debug)]
pub struct Opts {
    pub tab: String,
    pub tab_w: usize,
    pub width: usize,
    pub eol: String,
    pub eq: String,
    pub trail: bool,
    pub pad: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { tab: "\t".into(), tab_w: 4, width: 120, eol: "\r\n".into(), eq: " = ".into(), trail: true, pad: true }
    }
}

pub fn proto(p: &Proto, f32: bool, o: &Opts) -> Res<String> {
    let f = fun::make(p, 0, &[], &fun::Ctx::new(f32))?;
    let mut w = out::Out::new(o, f32);
    f.print(&mut w)?;
    Ok(w.done())
}

pub fn source_with(c: &Chunk, o: &Opts) -> Res<String> {
    proto(&c.main, c.h.num_sz == 4 && !c.h.integral, o)
}

pub fn source(c: &Chunk) -> Res<String> {
    source_with(c, &Opts::default())
}

pub fn decompile(b: &[u8]) -> Res<String> {
    source(&luac::parse(b)?)
}

#[cfg(test)]
mod gen;

#[cfg(test)]
fn same(a: &Proto, b: &Proto) -> bool {
    a.code == b.code && a.consts == b.consts && (a.params, a.vararg, a.nups) == (b.params, b.vararg, b.nups) && a.protos.len() == b.protos.len() && a.protos.iter().zip(&b.protos).all(|(x, y)| same(x, y))
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::comp::compile;
    use crate::luac::{parse, Header};

    const SRC: &str = r#"
local a, b, c = 1, "two", {x = 1, y = {2, 3}, [4] = 5, "list"}
local function f(x, y, ...)
    local t = {...}
    if x and y or not x then
        return x + y * 2 ^ -x, #t
    elseif x == y then
        x = x .. "s" .. y
    else
        while x < 10 do
            x = x + 1
            if x > 5 then break end
        end
    end
    for i = 1, 10, 2 do t[i] = i end
    for k, v in pairs(t) do print(k, v) end
    repeat y = y - 1 until y <= 0
    return f(x - 1, y)
end
function obj:method(n) self.n = (self.n or 0) + n return self end
local s = [[multi
line]] .. "esc\t\"q\"\0"
local u = a > 1 and b or c
do local z = f(1, 2) print(z) end
print(a, b, c, f, s, u, 1e300, 0.5, -3)
"#;

    const CASES: &str = r#"while f() do end
--
x = 1e999 - 1e999 y = -1e999
--
a, b = f(), 1, 2, 3, f()
--
local a = g() .. "x", {}
print(a)
--
local x, y, z = 1, 2, 3
x, y, z = nil
print(x, y, z)
--
local a, b = 1, 2
a, b = nil, nil
print(a, b)
--
f(w or y and z)
--
local v = w or y and z
print(v)
--
local x, y = a, b and c
print(x, y)
--
local x, y = a, b >= 3
print(x, y)
--
x = (a < b) == c and (a > b) == d
--
x = e and (a > b) == d and g
--
x = (a or b) and (c or d)
--
local a, b = g, h
if c then f0(1, not a and b or g10) end
--
local p = f()
g7[l949] = (p or g6) and g10 or f0(g3)
--
g10, g9, t[69] = (g3 ~= g4 or a or b) and f2(38, c), c.k1
--
local a, b
f((a or b and g0) and g8, x)
--
local u = 1
local f = function() return u, f end
--
local function f() return f end
--
local function h(...)
  repeat local h = function(p) return h end until g0
end
--
while a do
  if b then f() break end
  repeat x = 1 until f1()
end
--
repeat
  repeat local x = f() until x
until b
--
local u = 1
local t = {}, function() return u end
print(t)
--
if c or d then
  while f2(g, {k = t.x, "s"}) do end
end
--
if c then
  local v = a ~= "s" or g
else
  f()
end
--
for k, v in pairs(t) do
  if k then local x = g1 or v else end
end
--
for k, v in pairs(t) do
  local w = v and k or c and d
end
--
repeat
  do local a, b = g7 + 5, g3 or f1() end
until g11
--
while c do
  repeat
    local x = f()
    local g = function() return x end
  until d
end
--
for i = 1, 3 do
  repeat
    local x = f(i)
    h(function() return x end)
  until x > i
  print(i)
end
--
if c then
  while a do
    local x = f()
    g(function() return x end)
    if x then break end
  end
elseif d then
end
--
while a do
  repeat f() if x then break end g() until y
end
--
while a do
  if x then break end
end
while b do f() end
--
local u = 1
while f(function() return u end) do end
--
while c do
  local a, b = f, not g1 or g4
end
--
local x = f()
t.s, x = 1, not y or x
print(x)
--
for k, v in pairs(g5) do
  local a, b, c = {1}, "s"
  do local d, e, f end
  while x do end
end
--
do local a end
local function g() end
--
if g9 then
  local l = 1
else
  while (g1 or g9) and g5 < g9 do
    if c then break end
  end
end"#;

    fn rt(src: &str) -> String {
        let bin = compile(src.as_bytes(), "=t", Header::default(), 5).unwrap();
        let c = parse(&bin).unwrap();
        let out = source(&c).unwrap();
        let back = parse(&compile(out.as_bytes(), "=t", Header::default(), 5).unwrap()).unwrap();
        assert!(same(&back.main, &c.main), "{out}");
        out
    }

    #[test]
    fn roundtrips() {
        rt(SRC);
        CASES.split("\n--\n").for_each(|s| {
            rt(s);
        });
        for s in ["return", "local x x = 1", "a.b.c = d[e]", "local t = {f()} return ...", "for i = 3, 1, -1 do end", "x = function() end"] {
            rt(s);
        }
        let o = rt("function g(a) return a end");
        assert!(o.contains("function g(a)\r\n\treturn a\r\nend"), "{o}");
    }

    #[test]
    fn options() {
        let c = parse(&compile(b"if x then y = {1, 2} end", "=t", Header::default(), 5).unwrap()).unwrap();
        let o = Opts { tab: "  ".into(), tab_w: 2, width: 0, eol: "\n".into(), eq: "=".into(), trail: false, pad: false };
        assert_eq!(source_with(&c, &o).unwrap(), "if x then\n  y={1, 2}\nend\n");
        assert_eq!(source(&c).unwrap(), "if x then\r\n\ty = { 1, 2 }\r\nend\r\n");
        let c = parse(&compile(b"t = {a = 1, b = 2, c = {3, 4}, 5}", "=t", Header::default(), 5).unwrap()).unwrap();
        assert_eq!(source(&c).unwrap(), "t = {\r\n\ta = 1,\r\n\tb = 2,\r\n\tc = { 3, 4 },\r\n\t5,\r\n}\r\n");
        let long = format!("print({})", (0..60).map(|i| format!("v{i}")).collect::<Vec<_>>().join(", "));
        let c = parse(&compile(long.as_bytes(), "=t", Header::default(), 5).unwrap()).unwrap();
        assert_eq!(source_with(&c, &o).unwrap().lines().count(), 1);
        assert!(source(&c).unwrap().lines().count() > 10);
    }

    #[test]
    fn rejects() {
        assert!(decompile(b"junk").is_err());
        let mut c = parse(&compile(b"local x = 1 return x", "=t", Header::default(), 5).unwrap()).unwrap();
        c.main.code.pop();
        assert!(source(&c).is_err());
    }

    #[test]
    fn fuzz() {
        let c = parse(&compile(SRC.as_bytes(), "=t", Header::default(), 5).unwrap()).unwrap();
        let mut s = 0x9e3779b97f4a7c15u64;
        let mut rnd = |n: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n.max(1) as u64) as usize
        };
        let o = Opts::default();
        for _ in 0..20000 {
            let mut m = c.main.clone();
            for _ in 0..1 + rnd(3) {
                let p = if m.protos.is_empty() || rnd(2) == 0 { &mut m } else { let i = rnd(m.protos.len()); &mut m.protos[i] };
                match rnd(6) {
                    0 | 1 => {
                        let i = rnd(p.code.len());
                        p.code[i] ^= 1 << rnd(32);
                    }
                    2 => {
                        let i = rnd(p.code.len());
                        p.code[i] = (rnd(1 << 30) as u32) << 2 | rnd(4) as u32;
                    }
                    3 if !p.locals.is_empty() => {
                        let i = rnd(p.locals.len());
                        p.locals[i].start = rnd(p.code.len() + 3) as u64;
                        p.locals[i].end = rnd(p.code.len() + 3) as u64;
                    }
                    4 => p.stack = rnd(12) as u8,
                    _ => p.params = rnd(4) as u8,
                }
            }
            let _ = proto(&m, false, &o);
        }
    }
}

#[cfg(test)]
mod real {
    use crate::comp::compile;
    use super::same;
    use crate::luac::{parse, Proto};
    use std::path::Path;

    fn run(dir: &Path, ver: u8) -> (usize, Vec<String>) {
        let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "luac")).collect();
        v.sort();
        let mut bad = Vec::new();
        for p in &v {
            let c = parse(&std::fs::read(p).unwrap()).unwrap();
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            if let (Ok(o), Ok(s)) = (std::env::var("AC_DUMP"), super::source(&c)) {
                std::fs::write(Path::new(&o).join(format!("{ver}_{n}.lua")), s).unwrap();
            }
            match super::source(&c).and_then(|s| compile(s.as_bytes(), "=x", c.h, ver)) {
                Ok(b) if same(&parse(&b).unwrap().main, &c.main) => {}
                Ok(_) => bad.push(format!("{n}: differs")),
                Err(e) => bad.push(format!("{n}: {e}")),
            }
        }
        (v.len(), bad)
    }

    #[test]
    #[ignore]
    fn roundtrip() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let g = Path::new(&g);
        let mut all = Vec::new();
        for (d, ver) in [("lua", 5), ("game", 4)] {
            let (n, bad) = run(&g.join(d), ver);
            println!("{d}: {}/{n}", n - bad.len());
            bad.iter().take(10).for_each(|b| println!("  {b}"));
            all.extend(bad);
        }
        let known = ["t_attrib.luac: differs", "t_constructs.luac: differs", "t_db.luac: differs", "t_vararg.luac: differs"];
        assert!(all.iter().all(|b| known.contains(&b.as_str())), "{all:?}");
    }

    thread_local! {
        static BAD: std::cell::RefCell<Vec<(Proto, Proto)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn count(p: &Proto) -> usize {
        1 + p.protos.iter().map(count).sum::<usize>()
    }

    fn pair(a: &Proto, b: &Proto) -> usize {
        let me = (a.code == b.code && a.consts == b.consts) as usize;
        if me == 0 {
            BAD.with(|v| v.borrow_mut().push((a.clone(), b.clone())));
        }
        me + if a.protos.len() == b.protos.len() { a.protos.iter().zip(&b.protos).map(|(x, y)| pair(x, y)).sum() } else { 0 }
    }

    fn strip(p: &mut Proto) {
        p.lines.clear();
        p.locals.clear();
        p.ups.clear();
        p.protos.iter_mut().for_each(strip);
    }

    fn walk(p: &Proto, path: String, d: usize, cx: &std::rc::Rc<super::fun::Ctx>) {
        let only = std::env::var("AC_ONLY").ok();
        if only.as_ref().is_none_or(|o| *o == path) {
            match super::fun::make(p, d, &[], cx) {
                Ok(f) if super::fun::score(p, &f, d, cx) == usize::MAX => {}
                Ok(f) => {
                    let o = super::Opts::default();
                    let mut w = super::out::Out::new(&o, false);
                    let _ = f.print(&mut w);
                    println!("== {path} score {}
{}{}", super::fun::score(p, &f, d, cx), crate::luac::disasm(p), w.done());
                }
                Err(e) => println!("== {path} err {e}"),
            }
        }
        for (i, c) in p.protos.iter().enumerate() {
            walk(c, format!("{path}/{i}"), d + 1, cx);
        }
    }

    #[test]
    #[ignore]
    fn one() {
        let Ok(f) = std::env::var("AC_FILE") else { return };
        let mut c = parse(&std::fs::read(f).unwrap()).unwrap();
        strip(&mut c.main);
        walk(&c.main, "m".into(), 0, &super::fun::Ctx::new(false));
    }

    #[test]
    #[ignore]
    fn stripped() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let show: usize = std::env::var("AC_SHOW").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
        for (d, ver) in [("lua", 5), ("game", 4)] {
            let mut v: Vec<_> = std::fs::read_dir(Path::new(&g).join(d)).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "luac")).collect();
            v.sort();
            let one = |p: &std::path::PathBuf| {
                let mut c = parse(&std::fs::read(p).unwrap()).unwrap();
                strip(&mut c.main);
                if let (Ok(o), Ok(s)) = (std::env::var("AC_DUMP"), super::source(&c)) {
                    std::fs::write(Path::new(&o).join(format!("s{ver}_{}.lua", p.file_name().unwrap().to_string_lossy())), s).unwrap();
                }
                let n = count(&c.main);
                let r = match super::source(&c).and_then(|s| compile(s.as_bytes(), "=x", c.h, ver)) {
                    Ok(b) if same(&parse(&b).unwrap().main, &c.main) => (true, n, None),
                    Ok(b) => (false, pair(&parse(&b).unwrap().main, &c.main), Some("differs".to_string())),
                    Err(e) => (false, 0, Some(e.to_string())),
                };
                (r.0, r.1, n, r.2.map(|m| format!("  {}: {m}", p.file_name().unwrap().to_string_lossy())), BAD.with(|b| std::mem::take(&mut *b.borrow_mut())))
            };
            let k = std::thread::available_parallelism().map_or(4, |x| x.get());
            let mut res: Vec<_> = std::thread::scope(|s| {
                let hs: Vec<_> = (0..k).map(|t| { let (v, one) = (&v, &one); s.spawn(move || v.iter().enumerate().skip(t).step_by(k).map(|(i, p)| (i, one(p))).collect::<Vec<_>>()) }).collect();
                hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
            });
            res.sort_by_key(|x| x.0);
            let (ok, fm, ft) = res.iter().fold((0, 0, 0), |a, (_, r)| (a.0 + r.0 as usize, a.1 + r.1, a.2 + r.2));
            res.iter().filter_map(|(_, r)| r.3.as_ref()).take(show).for_each(|m| println!("{m}"));
            println!("{d} stripped: files {ok}/{}, functions {fm}/{ft}", v.len());
            {
                let mut v: Vec<(Proto, Proto)> = res.into_iter().flat_map(|(_, r)| r.4).collect();
                v.sort_by_key(|x| x.1.code.len());
                for (a, b) in v.iter().take(show) {
                    println!("---- stripped orig params {} vararg {}
{}---- recompiled
{}---- source
{}", b.params, b.vararg, crate::luac::disasm(b), crate::luac::disasm(a), super::proto(b, false, &super::Opts::default()).unwrap_or_default());
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn fuzz_corpus() {
        let Ok(g) = std::env::var("AC_GOLDEN") else { return };
        let mut s = 0x2545f4914f6cdd1du64;
        let mut rnd = |n: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n.max(1) as u64) as usize
        };
        let o = super::Opts::default();
        for e in std::fs::read_dir(Path::new(&g).join("game")).unwrap().flatten() {
            let c = parse(&std::fs::read(e.path()).unwrap()).unwrap();
            for _ in 0..30 {
                let mut m = c.main.clone();
                for _ in 0..1 + rnd(4) {
                    let mut p = &mut m;
                    while !p.protos.is_empty() && rnd(3) == 0 {
                        let i = rnd(p.protos.len());
                        p = &mut p.protos[i];
                    }
                    let i = rnd(p.code.len());
                    p.code[i] ^= 1 << rnd(32);
                }
                let _ = super::proto(&m, true, &o);
            }
        }
    }
}
