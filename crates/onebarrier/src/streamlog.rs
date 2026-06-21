//! M3 (Track A), App #5 — a durable, totally-ordered **pub/sub streaming log**
//! (the Storm/Kafka streaming class). Built entirely by orchestrating the engine's
//! existing ops on the shared [`crate::server::KvService`] — publish appends at a
//! monotonic offset (INCR), messages are stored (SET), consumers replay a range
//! (GET). Durability + crash recovery come from the engine for free.
//!
//! Line protocol: `PUB <topic> <msg>` | `SUB <topic> <from>` | `LEN <topic>` | `QUIT`

use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::{is_timeout, setup_stream, KvService, Protocol};
use crate::{Op, Output};

#[derive(Debug, Default)]
pub struct StreamLogProtocol;

impl Protocol for StreamLogProtocol {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()> {
        setup_stream(&stream);
        let mut reader = io::BufReader::new(stream.try_clone()?);
        let mut writer = io::BufWriter::new(stream);
        let mut seq: u64 = 0;
        let mut line = String::new();
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
            let parts: Vec<&str> = line.splitn(3, ' ').map(str::trim_end).collect();
            if parts.is_empty() || parts[0].is_empty() {
                continue;
            }
            match parts[0].to_ascii_uppercase().as_str() {
                "PUB" if parts.len() >= 3 => {
                    let topic = parts[1];
                    let msg = parts[2].as_bytes();
                    // Append at the next monotonic offset (INCR), then store it.
                    seq += 1;
                    let offset = match svc.apply(Op::incr(conn_id, seq, &format!("t:{topic}:n"), 1)) {
                        Some(Output::Value(Some(n))) => n,
                        _ => {
                            writer.write_all(b"ERR unavailable\r\n")?;
                            writer.flush()?;
                            continue;
                        }
                    };
                    seq += 1;
                    svc.apply(Op::set_bytes(conn_id, seq, &format!("t:{topic}:{offset}"), msg));
                    write!(writer, "OFFSET {offset}\r\n")?;
                }
                "SUB" if parts.len() >= 3 => {
                    let topic = parts[1];
                    let from: i64 = parts[2].trim().parse().unwrap_or(1);
                    seq += 1;
                    let len = match svc.apply(Op::get(conn_id, seq, &format!("t:{topic}:n"))) {
                        Some(Output::Bytes(Some(v))) => std::str::from_utf8(&v).ok().and_then(|s| s.parse().ok()).unwrap_or(0),
                        _ => 0i64,
                    };
                    for off in from.max(1)..=len {
                        seq += 1;
                        if let Some(Output::Bytes(Some(m))) = svc.apply(Op::get(conn_id, seq, &format!("t:{topic}:{off}"))) {
                            write!(writer, "MSG {off} {}\r\n", m.len())?;
                            writer.write_all(&m)?;
                            writer.write_all(b"\r\n")?;
                        }
                    }
                    writer.write_all(b"END\r\n")?;
                }
                "LEN" if parts.len() >= 2 => {
                    let topic = parts[1];
                    seq += 1;
                    let len = match svc.apply(Op::get(conn_id, seq, &format!("t:{topic}:n"))) {
                        Some(Output::Bytes(Some(v))) => std::str::from_utf8(&v).ok().and_then(|s| s.parse().ok()).unwrap_or(0),
                        _ => 0i64,
                    };
                    write!(writer, "LEN {len}\r\n")?;
                }
                "QUIT" => return Ok(()),
                _ => writer.write_all(b"ERR unknown command\r\n")?,
            }
            writer.flush()?;
        }
    }
}

fn read_line<R: BufRead>(r: &mut R, out: &mut String) -> io::Result<usize> {
    let mut raw = Vec::new();
    let n = r.read_until(b'\n', &mut raw)?;
    *out = String::from_utf8_lossy(&raw).trim_end_matches(['\r', '\n']).to_string();
    Ok(n)
}

pub fn start_streamlog(cfg: crate::server::ServerConfig) -> io::Result<crate::server::ServerHandle> {
    crate::server::start(cfg, std::sync::Arc::new(StreamLogProtocol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ServerConfig;
    use std::io::Read;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU64;

    fn tmpdir(tag: &str) -> PathBuf {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("ob-log-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn req(s: &mut TcpStream, bytes: &str) -> String {
        s.write_all(bytes.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn streamlog_publish_consume_and_recovers() {
        let dir = tmpdir("log");
        let cfg = |d: &PathBuf| ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
            dir: d.clone(),
            snap_interval: 1000,
            fsync: false,
        };
        {
            let h = start_streamlog(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            assert_eq!(req(&mut s, "PUB events alpha\r\n"), "OFFSET 1\r\n");
            assert_eq!(req(&mut s, "PUB events beta\r\n"), "OFFSET 2\r\n");
            assert_eq!(req(&mut s, "PUB events gamma\r\n"), "OFFSET 3\r\n");
            assert_eq!(req(&mut s, "LEN events\r\n"), "LEN 3\r\n");
            let sub = req(&mut s, "SUB events 1\r\n");
            assert!(sub.contains("MSG 1 5\r\nalpha") && sub.contains("MSG 3 5\r\ngamma") && sub.ends_with("END\r\n"), "SUB: {sub:?}");
            drop(s);
            h.stop();
        }
        // recovery: the log persists in total order
        let h = start_streamlog(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        assert_eq!(req(&mut s, "LEN events\r\n"), "LEN 3\r\n", "log recovered");
        let sub = req(&mut s, "SUB events 2\r\n");
        assert!(sub.contains("MSG 2 4\r\nbeta") && sub.contains("MSG 3 5\r\ngamma"), "recovered SUB: {sub:?}");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
