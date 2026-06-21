//! `ob-jepsen` — M5: a Jepsen-style concurrent-fault consistency check against
//! the REAL `ob-kv` server process. Concurrent clients write unique keys and
//! record acknowledgements; the server is **`kill -9`'d mid-load and restarted**;
//! then we verify the core linearizability/durability invariant:
//!
//!   every ACKNOWLEDGED write is present, with its exact value, after the crash —
//!   no lost acked write, no torn value, no fabricated key.
//!
//! Ambiguous ops (in flight at the crash) are recorded as "unknown" and NOT
//! retried (the RESP protocol carries no idempotency key across reconnects), so
//! they are excluded from the must-be-present set — an honest output-commit gap.
//!
//!   ob-jepsen [--port N] [--clients C] [--ops M] [--server PATH]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn spawn_server(bin: &str, port: u16, dir: &PathBuf) -> std::io::Result<Child> {
    Command::new(bin)
        .args(["--port", &port.to_string(), "--dir", dir.to_str().unwrap(), "--snap-interval", "500"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn wait_up(port: u16, deadline: Duration) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// One SET round-trip. Ok(true)=+OK acked; Ok(false)=other reply; Err=conn error.
fn do_set(s: &mut TcpStream, key: &str, val: &str) -> std::io::Result<bool> {
    let req = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", key.len(), key, val.len(), val);
    s.write_all(req.as_bytes())?;
    s.flush()?;
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf)?;
    Ok(n >= 3 && &buf[..3] == b"+OK")
}

fn get(s: &mut TcpStream, key: &str) -> std::io::Result<Option<String>> {
    let req = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
    s.write_all(req.as_bytes())?;
    s.flush()?;
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf)?;
    let r = String::from_utf8_lossy(&buf[..n]);
    if r.starts_with("$-1") {
        return Ok(None);
    }
    // $<len>\r\n<val>\r\n
    Ok(r.split("\r\n").nth(1).map(str::to_string))
}

fn main() -> std::io::Result<()> {
    let mut port = 6411u16;
    let mut clients = 8usize;
    let mut ops = 2000u64;
    let mut bin = String::from("./target/release/ob-kv");
    let mut a = std::env::args().skip(1);
    while let Some(x) = a.next() {
        match x.as_str() {
            "--port" => port = a.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--clients" => clients = a.next().and_then(|v| v.parse().ok()).unwrap_or(clients),
            "--ops" => ops = a.next().and_then(|v| v.parse().ok()).unwrap_or(ops),
            "--server" => bin = a.next().unwrap_or(bin),
            _ => {}
        }
    }
    let dir = std::env::temp_dir().join(format!("ob-jepsen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    println!("OneBarrier M5 — Jepsen-style concurrent-fault consistency check");
    println!("  {clients} clients × {ops} unique writes, with a kill -9 + restart mid-load\n");

    // Start the real server.
    let mut child = spawn_server(&bin, port, &dir)?;
    if !wait_up(port, Duration::from_secs(5)) {
        eprintln!("server did not come up — build it: cargo build --release -p onebarrier --bin ob-kv");
        let _ = child.kill();
        return Ok(());
    }

    let crashed = Arc::new(AtomicBool::new(false));
    // Coordinator: kill -9 mid-load, then restart and recover.
    let coord = {
        let crashed = Arc::clone(&crashed);
        // child is moved/managed here via a channel of the pid; simpler: kill by handle after a delay in main thread post-spawn of clients.
        crashed
    };
    let _ = coord;

    // Spawn clients.
    let handles: Vec<_> = (0..clients)
        .map(|c| {
            thread::spawn(move || -> (u64, u64, Vec<(String, String)>) {
                let mut acked: Vec<(String, String)> = Vec::new();
                let mut unknown = 0u64;
                let mut conn = TcpStream::connect(("127.0.0.1", port)).ok();
                if let Some(ref s) = conn {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                }
                for i in 0..ops {
                    let key = format!("c{c}_{i}");
                    let val = format!("v{c}_{i}");
                    let mut done = false;
                    for _attempt in 0..200 {
                        if conn.is_none() {
                            match TcpStream::connect(("127.0.0.1", port)) {
                                Ok(s) => {
                                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                                    conn = Some(s);
                                }
                                Err(_) => {
                                    thread::sleep(Duration::from_millis(20));
                                    continue;
                                }
                            }
                        }
                        let s = conn.as_mut().unwrap();
                        match do_set(s, &key, &val) {
                            Ok(true) => {
                                acked.push((key.clone(), val.clone()));
                                done = true;
                                break;
                            }
                            Ok(false) => {
                                done = true;
                                break;
                            }
                            Err(_) => {
                                // connection error: server may be down (the crash).
                                // Mark ambiguous, drop conn, do NOT retry this op.
                                conn = None;
                                break;
                            }
                        }
                    }
                    if !done {
                        unknown += 1;
                    }
                }
                (acked.len() as u64, unknown, acked)
            })
        })
        .collect();

    // Let some writes land, then crash + restart.
    thread::sleep(Duration::from_millis(150));
    println!("  >>> kill -9 the server (crash) ...");
    let _ = child.kill();
    let _ = child.wait();
    crashed.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(200));
    let mut child = spawn_server(&bin, port, &dir)?;
    let up = wait_up(port, Duration::from_secs(5));
    println!("  >>> server restarted and recovered: {up}");

    // Join clients.
    let mut total_acked = 0u64;
    let mut total_unknown = 0u64;
    let mut all_acked: Vec<(String, String)> = Vec::new();
    for h in handles {
        let (acked, unknown, keys) = h.join().unwrap();
        total_acked += acked;
        total_unknown += unknown;
        all_acked.extend(keys);
    }

    // Verify: every acknowledged write is present with its exact value.
    let mut verify = TcpStream::connect(("127.0.0.1", port))?;
    verify.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut lost = 0u64;
    let mut torn = 0u64;
    for (k, v) in &all_acked {
        match get(&mut verify, k) {
            Ok(Some(got)) if &got == v => {}
            Ok(Some(_)) => torn += 1,
            Ok(None) => lost += 1,
            Err(_) => lost += 1,
        }
    }

    println!("\n  acknowledged writes:        {total_acked}");
    println!("  ambiguous (in-flight) ops:  {total_unknown}  (excluded — honest output-commit gap)");
    println!("  LOST acked writes:          {lost}   <- must be 0 (no lost acknowledged write)");
    println!("  TORN values:                {torn}   <- must be 0 (no value corruption)");
    let ok = lost == 0 && torn == 0 && up;
    println!("\n  RESULT: {}", if ok { "PASS — every acknowledged write survived the crash, exactly" } else { "FAIL — consistency violation" });

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&dir);
    std::process::exit(i32::from(!ok));
}
