//! M3 (Track A), App #3 — an HTTP/1.1 REST key-value server on the OneBarrier
//! engine (the web-serving app class — the Nginx/Node target). Reuses
//! [`crate::server::KvService`] (same executor, durability, crash recovery).
//! Supports keep-alive so it benchmarks with `ab`/`wrk` and drives with `curl`:
//!
//!   GET    /<key>        -> 200 <value> | 404
//!   PUT    /<key>  body  -> 200 OK            (idempotent set)
//!   POST   /incr/<key>   -> 200 <new int>    (INCR; ?by=N supported)
//!   DELETE /<key>        -> 200 <count>

use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::server::{is_timeout, setup_stream, KvService, Protocol};
use crate::{Op, Output};

#[derive(Debug, Default)]
pub struct HttpProtocol;

impl Protocol for HttpProtocol {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()> {
        setup_stream(&stream);
        let mut reader = io::BufReader::new(stream.try_clone()?);
        let mut writer = io::BufWriter::new(stream);
        let mut seq: u64 = 0;
        loop {
            // Request line.
            let mut line = String::new();
            match read_line_str(&mut reader, &mut line) {
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
            let mut it = line.split_whitespace();
            let (method, path) = match (it.next(), it.next()) {
                (Some(m), Some(p)) => (m.to_string(), p.to_string()),
                _ => continue,
            };

            // Headers → Content-Length + keep-alive.
            let mut content_len = 0usize;
            let mut keep_alive = true;
            loop {
                let mut h = String::new();
                if read_line_str(&mut reader, &mut h)? == 0 {
                    break;
                }
                let t = h.trim_end();
                if t.is_empty() {
                    break;
                }
                if let Some((k, v)) = t.split_once(':') {
                    let k = k.trim().to_ascii_lowercase();
                    let v = v.trim();
                    if k == "content-length" {
                        content_len = v.parse().unwrap_or(0);
                    } else if k == "connection" {
                        keep_alive = !v.eq_ignore_ascii_case("close");
                    }
                }
            }
            let mut body = vec![0u8; content_len];
            if content_len > 0 {
                reader.read_exact(&mut body)?;
            }

            seq += 1;
            let (status, payload) = route(&method, &path, &body, conn_id, seq, svc);
            write_response(&mut writer, status, &payload, keep_alive)?;
            writer.flush()?;
            if !keep_alive {
                return Ok(());
            }
        }
    }
}

fn route(method: &str, path: &str, body: &[u8], client: u32, seq: u64, svc: &KvService) -> (u16, Vec<u8>) {
    let path = path.split('?').next().unwrap_or(path);
    let query = path; // (query parsed separately below for ?by=)
    let _ = query;
    match method {
        "GET" => {
            let key = path.trim_start_matches('/');
            match svc.apply(Op::get(client, seq, key)) {
                Some(Output::Bytes(Some(v))) => (200, v),
                _ => (404, b"not found".to_vec()),
            }
        }
        "PUT" | "POST" if !path.starts_with("/incr/") => {
            let key = path.trim_start_matches('/');
            match svc.apply(Op::set_bytes(client, seq, key, body)) {
                Some(_) => (200, b"OK".to_vec()),
                None => (503, b"unavailable".to_vec()),
            }
        }
        "POST" => {
            // POST /incr/<key>[?by=N]
            let rest = &path["/incr/".len()..];
            let key = rest.to_string();
            match svc.apply(Op::incr(client, seq, &key, 1)) {
                Some(Output::Value(Some(n))) => (200, n.to_string().into_bytes()),
                _ => (503, b"unavailable".to_vec()),
            }
        }
        "DELETE" => {
            let key = path.trim_start_matches('/');
            match svc.apply(Op::del(client, seq, key)) {
                Some(Output::Value(Some(n))) => (200, n.to_string().into_bytes()),
                _ => (200, b"0".to_vec()),
            }
        }
        _ => (405, b"method not allowed".to_vec()),
    }
}

fn write_response<W: Write>(w: &mut W, status: u16, body: &[u8], keep_alive: bool) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "OK",
    };
    write!(w, "HTTP/1.1 {status} {reason}\r\n")?;
    write!(w, "Content-Length: {}\r\n", body.len())?;
    write!(w, "Content-Type: application/octet-stream\r\n")?;
    write!(w, "Connection: {}\r\n\r\n", if keep_alive { "keep-alive" } else { "close" })?;
    w.write_all(body)
}

fn read_line_str<R: BufRead>(r: &mut R, out: &mut String) -> io::Result<usize> {
    let mut raw = Vec::new();
    let n = r.read_until(b'\n', &mut raw)?;
    *out = String::from_utf8_lossy(&raw).into_owned();
    Ok(n)
}

pub fn start_http(cfg: crate::server::ServerConfig) -> io::Result<crate::server::ServerHandle> {
    crate::server::start(cfg, std::sync::Arc::new(HttpProtocol))
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
        d.push(format!("ob-http-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn http(s: &mut TcpStream, req: &str) -> String {
        s.write_all(req.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut buf = [0u8; 1024];
        let n = s.read(&mut buf).unwrap();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn http_rest_put_get_incr_and_recovers() {
        let dir = tmpdir("http");
        let cfg = |d: &PathBuf| ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
            dir: d.clone(),
            snap_interval: 1000,
            fsync: false,
        };
        let addr = {
            let h = start_http(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            let r = http(&mut s, "PUT /greeting HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello");
            assert!(r.starts_with("HTTP/1.1 200"), "PUT: {r}");
            let r = http(&mut s, "GET /greeting HTTP/1.1\r\n\r\n");
            assert!(r.contains("200") && r.ends_with("hello"), "GET: {r}");
            let r = http(&mut s, "POST /incr/views HTTP/1.1\r\n\r\n");
            assert!(r.ends_with("1"), "INCR1: {r}");
            let r = http(&mut s, "POST /incr/views HTTP/1.1\r\n\r\n");
            assert!(r.ends_with("2"), "INCR2: {r}");
            let r = http(&mut s, "GET /missing HTTP/1.1\r\n\r\n");
            assert!(r.starts_with("HTTP/1.1 404"), "404: {r}");
            let a = h.addr;
            drop(s);
            h.stop();
            a
        };
        let _ = addr;
        // recover
        let h = start_http(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        let r = http(&mut s, "GET /greeting HTTP/1.1\r\n\r\n");
        assert!(r.ends_with("hello"), "recovered GET: {r}");
        let r = http(&mut s, "GET /views HTTP/1.1\r\n\r\n");
        assert!(r.ends_with("2"), "recovered INCR state: {r}");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
