use ac_core::{bad, Error, Res};
use std::io::{BufRead, ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub fn resolve(addr: &str) -> Res<Vec<SocketAddr>> {
    let v: Vec<SocketAddr> = addr.to_socket_addrs()?.collect();
    if v.is_empty() {
        return bad("address did not resolve");
    }
    Ok(v)
}

pub fn dial(addr: &str, timeout: Duration) -> Res<TcpStream> {
    let mut last = Error::Bad("address did not resolve");
    for a in resolve(addr)? {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                s.set_nodelay(true)?;
                s.set_read_timeout(Some(timeout))?;
                s.set_write_timeout(Some(timeout))?;
                return Ok(s);
            }
            Err(e) => last = e.into(),
        }
    }
    Err(last)
}

pub fn listen(addr: &str) -> Res<TcpListener> {
    Ok(TcpListener::bind(addr)?)
}

pub fn send_frame(w: &mut impl Write, d: &[u8]) -> Res<()> {
    let n = u32::try_from(d.len()).map_err(|_| Error::Bad("frame too large"))?;
    w.write_all(&n.to_be_bytes())?;
    w.write_all(d)?;
    w.flush()?;
    Ok(())
}

pub fn recv_frame(r: &mut impl Read, max: usize) -> Res<Option<Vec<u8>>> {
    let mut h = [0u8; 4];
    match r.read_exact(&mut h) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let n = u32::from_be_bytes(h) as usize;
    if n > max {
        return bad("frame exceeds limit");
    }
    let mut d = vec![0; n];
    r.read_exact(&mut d)?;
    Ok(Some(d))
}

pub fn read_line(r: &mut impl BufRead, max: usize) -> Res<Option<String>> {
    let mut v = Vec::new();
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(if v.is_empty() { None } else { Some(String::from_utf8_lossy(&v).into_owned()) });
        }
        let (n, done) = match buf.iter().position(|&c| c == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        v.extend_from_slice(&buf[..n]);
        r.consume(n);
        if v.len() > max {
            return bad("line exceeds limit");
        }
        if done {
            while matches!(v.last(), Some(b'\n' | b'\r')) {
                v.pop();
            }
            return Ok(Some(String::from_utf8_lossy(&v).into_owned()));
        }
    }
}

pub fn udp(bind: &str, timeout: Duration) -> Res<UdpSocket> {
    let s = UdpSocket::bind(bind)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    Ok(s)
}

pub fn udp_query(addr: &str, d: &[u8], timeout: Duration, tries: u32) -> Res<Vec<u8>> {
    let to = resolve(addr)?[0];
    let s = udp(if to.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }, timeout)?;
    let mut buf = vec![0u8; 65536];
    for _ in 0..tries.max(1) {
        s.send_to(d, to)?;
        match s.recv_from(&mut buf) {
            Ok((n, from)) if from.ip() == to.ip() => return Ok(buf[..n].to_vec()),
            Ok(_) => continue,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bad("no reply")
}

pub fn local_ip() -> Res<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0")?;
    s.connect("192.0.2.1:9")?;
    Ok(s.local_addr()?.ip())
}

pub fn free_port() -> Res<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

#[cfg(test)]
mod t {
    use super::*;
    use std::io::{BufReader, Cursor};
    use std::thread;

    #[test]
    fn frames() {
        let l = listen("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap().to_string();
        let h = thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            while let Some(f) = recv_frame(&mut s, 1 << 20).unwrap() {
                let r: Vec<u8> = f.iter().rev().copied().collect();
                send_frame(&mut s, &r).unwrap();
            }
        });
        let mut c = dial(&a, Duration::from_secs(5)).unwrap();
        for m in [&b"hello"[..], b"", &[7u8; 70000]] {
            send_frame(&mut c, m).unwrap();
            let r = recv_frame(&mut c, 1 << 20).unwrap().unwrap();
            assert_eq!(r, m.iter().rev().copied().collect::<Vec<u8>>());
        }
        drop(c);
        h.join().unwrap();
        assert!(recv_frame(&mut Cursor::new(vec![0, 0, 1, 0, 1]), 16).is_err());
        assert!(recv_frame(&mut Cursor::new(Vec::new()), 16).unwrap().is_none());
    }

    #[test]
    fn lines() {
        let mut r = BufReader::with_capacity(4, Cursor::new(b"ab\r\nlonger line\n\nlast".to_vec()));
        let v: Vec<String> = std::iter::from_fn(|| read_line(&mut r, 64).unwrap()).collect();
        assert_eq!(v, ["ab", "longer line", "", "last"]);
        assert!(read_line(&mut Cursor::new(vec![b'x'; 100]), 10).is_err());
    }

    #[test]
    fn udp_echo() {
        let s = udp("127.0.0.1:0", Duration::from_secs(5)).unwrap();
        let a = s.local_addr().unwrap().to_string();
        let h = thread::spawn(move || {
            let mut b = [0u8; 512];
            let (n, from) = s.recv_from(&mut b).unwrap();
            s.send_to(&b[..n].to_ascii_uppercase(), from).unwrap();
        });
        assert_eq!(udp_query(&a, b"ping", Duration::from_secs(5), 2).unwrap(), b"PING");
        h.join().unwrap();
        assert!(free_port().unwrap() > 0);
        assert!(dial("127.0.0.1:1", Duration::from_millis(500)).is_err());
    }
}
