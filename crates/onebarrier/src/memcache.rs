//! M3 (Track A), App #2 — a Memcached **text-protocol** server on the OneBarrier
//! engine, reusing [`crate::server::KvService`] (same single executor, same
//! durability + crash recovery). Speaks the classic ASCII protocol
//! (`set`/`get`/`gets`/`incr`/`decr`/`delete`/`version`/`quit`), so it is
//! drivable by `memtier_benchmark --protocol=memcache_text` and `memccat`/`nc`.
//!
//! Flags are accepted and reported as 0 (values are stored opaque); INCR/DECR
//! follow Redis-style integer semantics on the stored value.

use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::{is_timeout, setup_stream, KvService, Protocol};
use crate::{Op, Output};

#[derive(Debug, Default)]
pub struct MemcacheProtocol;

impl Protocol for MemcacheProtocol {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()> {
        setup_stream(&stream);
        let mut reader = io::BufReader::new(stream.try_clone()?);
        let mut writer = io::BufWriter::new(stream);
        let mut seq: u64 = 0;
        let mut line = Vec::new();
        loop {
            line.clear();
            match read_line(&mut reader, &mut line) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(ref e) if is_timeout(e) => {
                    if shutdown.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            }
            let parts: Vec<&[u8]> = line.split(|b| *b == b' ').filter(|s| !s.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }
            let cmd = parts[0].to_ascii_lowercase();
            match cmd.as_slice() {
                b"set" | b"add" | b"replace" => {
                    // set <key> <flags> <exptime> <bytes> [noreply]
                    if parts.len() < 5 {
                        writer.write_all(b"ERROR\r\n")?;
                        writer.flush()?;
                        continue;
                    }
                    let key = String::from_utf8_lossy(parts[1]).into_owned();
                    let nbytes = parse_usize(parts[4]);
                    let noreply = parts.last() == Some(&b"noreply".as_slice());
                    let mut data = vec![0u8; nbytes + 2]; // data + CRLF
                    reader.read_exact(&mut data)?;
                    data.truncate(nbytes);
                    seq += 1;
                    svc.apply(Op::set_bytes(conn_id, seq, &key, &data));
                    if !noreply {
                        writer.write_all(b"STORED\r\n")?;
                    }
                }
                b"get" | b"gets" => {
                    for k in &parts[1..] {
                        let key = String::from_utf8_lossy(k).into_owned();
                        seq += 1;
                        if let Some(Output::Bytes(Some(v))) = svc.apply(Op::get(conn_id, seq, &key)) {
                            write!(writer, "VALUE {} 0 {}\r\n", key, v.len())?;
                            writer.write_all(&v)?;
                            writer.write_all(b"\r\n")?;
                        }
                    }
                    writer.write_all(b"END\r\n")?;
                }
                b"incr" | b"decr" if parts.len() >= 3 => {
                    let key = String::from_utf8_lossy(parts[1]).into_owned();
                    let mag = parse_i64(parts[2]);
                    seq += 1;
                    let exists = matches!(svc.apply(Op::get(conn_id, seq, &key)), Some(Output::Bytes(Some(_))));
                    if !exists {
                        writer.write_all(b"NOT_FOUND\r\n")?;
                    } else {
                        let delta = if cmd == b"decr" { -mag } else { mag };
                        seq += 1;
                        if let Some(Output::Value(Some(n))) = svc.apply(Op::incr(conn_id, seq, &key, delta)) {
                            write!(writer, "{}\r\n", n.max(0))?;
                        } else {
                            writer.write_all(b"ERROR\r\n")?;
                        }
                    }
                }
                b"delete" if parts.len() >= 2 => {
                    let key = String::from_utf8_lossy(parts[1]).into_owned();
                    seq += 1;
                    match svc.apply(Op::del(conn_id, seq, &key)) {
                        Some(Output::Value(Some(1))) => writer.write_all(b"DELETED\r\n")?,
                        _ => writer.write_all(b"NOT_FOUND\r\n")?,
                    }
                }
                b"version" => writer.write_all(b"VERSION onebarrier-0.1\r\n")?,
                b"quit" => return Ok(()),
                _ => writer.write_all(b"ERROR\r\n")?,
            }
            writer.flush()?;
        }
    }
}

fn read_line<R: BufRead>(r: &mut R, out: &mut Vec<u8>) -> io::Result<usize> {
    let mut raw = Vec::new();
    let n = r.read_until(b'\n', &mut raw)?;
    if n == 0 {
        return Ok(0);
    }
    while matches!(raw.last(), Some(b'\n' | b'\r')) {
        raw.pop();
    }
    *out = raw;
    Ok(n)
}

fn parse_usize(b: &[u8]) -> usize {
    std::str::from_utf8(b).ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}
fn parse_i64(b: &[u8]) -> i64 {
    std::str::from_utf8(b).ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Convenience: start a Memcached-protocol server.
pub fn start_memcache(cfg: crate::server::ServerConfig) -> io::Result<crate::server::ServerHandle> {
    crate::server::start(cfg, std::sync::Arc::new(MemcacheProtocol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ServerConfig;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    fn tmpdir(tag: &str) -> PathBuf {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("ob-mc-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn req(s: &mut TcpStream, bytes: &[u8]) -> Vec<u8> {
        s.write_all(bytes).unwrap();
        s.flush().unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn memcache_set_get_delete_and_recovers() {
        let dir = tmpdir("mc");
        let cfg = |d: &PathBuf| ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
            dir: d.clone(),
            snap_interval: 1000,
            fsync: false,
        };
        {
            let h = start_memcache(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            assert_eq!(req(&mut s, b"set foo 0 0 3\r\nbar\r\n"), b"STORED\r\n");
            assert_eq!(req(&mut s, b"get foo\r\n"), b"VALUE foo 0 3\r\nbar\r\nEND\r\n");
            assert_eq!(req(&mut s, b"get missing\r\n"), b"END\r\n");
            assert_eq!(req(&mut s, b"set n 0 0 1\r\n5\r\n"), b"STORED\r\n");
            assert_eq!(req(&mut s, b"incr n 10\r\n"), b"15\r\n");
            assert_eq!(req(&mut s, b"delete foo\r\n"), b"DELETED\r\n");
            assert_eq!(req(&mut s, b"delete foo\r\n"), b"NOT_FOUND\r\n");
            drop(s);
            h.stop();
        }
        // recover
        let h = start_memcache(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        assert_eq!(req(&mut s, b"get foo\r\n"), b"END\r\n", "delete persisted");
        assert_eq!(req(&mut s, b"get n\r\n"), b"VALUE n 0 2\r\n15\r\nEND\r\n", "incr state recovered");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
