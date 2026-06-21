//! M3 (Track A), App #4 — a transactional key-value store (the database /
//! SQLite-class app) on the OneBarrier engine. Multi-key transactions commit
//! **atomically**: a `BEGIN … COMMIT` block is applied as one `Txn` op = one
//! totally-ordered, durably-logged unit, so it is all-or-nothing on commit *and*
//! on recovery. Reuses [`crate::server::KvService`].
//!
//! Line protocol (whitespace-tokenized; values are tokens):
//!   BEGIN | SET k v | DEL k | GET k | COMMIT | ABORT | QUIT
//! GET reads committed state (read-committed). Outside a txn, SET/DEL apply
//! immediately as a singleton atomic write.

use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::{is_timeout, setup_stream, KvService, Protocol};
use crate::{Op, Output};

#[derive(Debug, Default)]
pub struct TxnProtocol;

impl Protocol for TxnProtocol {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()> {
        setup_stream(&stream);
        let mut reader = io::BufReader::new(stream.try_clone()?);
        let mut writer = io::BufWriter::new(stream);
        let mut seq: u64 = 0;
        let mut buf: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        let mut in_txn = false;
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
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            match parts[0].to_ascii_uppercase().as_str() {
                "BEGIN" => {
                    in_txn = true;
                    buf.clear();
                    writer.write_all(b"BEGIN\r\n")?;
                }
                "SET" if parts.len() >= 3 => {
                    let entry = (parts[1].to_string(), Some(parts[2].as_bytes().to_vec()));
                    if in_txn {
                        buf.push(entry);
                        writer.write_all(b"QUEUED\r\n")?;
                    } else {
                        seq += 1;
                        svc.apply(Op::txn(conn_id, seq, vec![entry]));
                        writer.write_all(b"OK\r\n")?;
                    }
                }
                "DEL" if parts.len() >= 2 => {
                    let entry = (parts[1].to_string(), None);
                    if in_txn {
                        buf.push(entry);
                        writer.write_all(b"QUEUED\r\n")?;
                    } else {
                        seq += 1;
                        svc.apply(Op::txn(conn_id, seq, vec![entry]));
                        writer.write_all(b"OK\r\n")?;
                    }
                }
                "GET" if parts.len() >= 2 => {
                    seq += 1;
                    match svc.apply(Op::get(conn_id, seq, parts[1])) {
                        Some(Output::Bytes(Some(v))) => {
                            write!(writer, "VALUE {}\r\n", v.len())?;
                            writer.write_all(&v)?;
                            writer.write_all(b"\r\n")?;
                        }
                        _ => writer.write_all(b"NIL\r\n")?,
                    }
                }
                "COMMIT" => {
                    if !in_txn {
                        writer.write_all(b"ERR no transaction\r\n")?;
                    } else {
                        let n = buf.len();
                        seq += 1;
                        svc.apply(Op::txn(conn_id, seq, std::mem::take(&mut buf)));
                        in_txn = false;
                        write!(writer, "COMMIT {n}\r\n")?;
                    }
                }
                "ABORT" => {
                    buf.clear();
                    in_txn = false;
                    writer.write_all(b"ABORT\r\n")?;
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
    *out = String::from_utf8_lossy(&raw).into_owned();
    Ok(n)
}

pub fn start_txn(cfg: crate::server::ServerConfig) -> io::Result<crate::server::ServerHandle> {
    crate::server::start(cfg, std::sync::Arc::new(TxnProtocol))
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
        d.push(format!("ob-txn-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn req(s: &mut TcpStream, bytes: &str) -> String {
        s.write_all(bytes.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn atomic_transaction_commits_and_recovers() {
        let dir = tmpdir("txn");
        let cfg = |d: &PathBuf| ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
            dir: d.clone(),
            snap_interval: 1000,
            fsync: false,
        };
        {
            let h = start_txn(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            assert_eq!(req(&mut s, "BEGIN\r\n"), "BEGIN\r\n");
            assert_eq!(req(&mut s, "SET acct_a 100\r\n"), "QUEUED\r\n");
            assert_eq!(req(&mut s, "SET acct_b 0\r\n"), "QUEUED\r\n");
            // uncommitted: not visible yet (read-committed)
            assert_eq!(req(&mut s, "GET acct_a\r\n"), "NIL\r\n");
            assert_eq!(req(&mut s, "COMMIT\r\n"), "COMMIT 2\r\n");
            // now visible, atomically
            assert_eq!(req(&mut s, "GET acct_a\r\n"), "VALUE 3\r\n100\r\n");
            // abort discards
            assert_eq!(req(&mut s, "BEGIN\r\n"), "BEGIN\r\n");
            assert_eq!(req(&mut s, "SET acct_a 999\r\n"), "QUEUED\r\n");
            assert_eq!(req(&mut s, "ABORT\r\n"), "ABORT\r\n");
            assert_eq!(req(&mut s, "GET acct_a\r\n"), "VALUE 3\r\n100\r\n", "abort discarded");
            drop(s);
            h.stop();
        }
        // recover: the committed transaction is durable and atomic
        let h = start_txn(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        assert_eq!(req(&mut s, "GET acct_a\r\n"), "VALUE 3\r\n100\r\n", "txn recovered");
        assert_eq!(req(&mut s, "GET acct_b\r\n"), "VALUE 1\r\n0\r\n", "txn recovered atomically");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
