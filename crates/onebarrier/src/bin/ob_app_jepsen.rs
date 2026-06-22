//! `ob-app-jepsen` — the correctness torture test applied to an UNMODIFIED app.
//!
//! ob-jepsen / ob-lincheck exercise the OneBarrier *engine*. This binary runs the
//! same adversarial checks against a stock **redis-server** recovered through the
//! libOS path: concurrent clients hammer redis (running under the obpreload shim,
//! which captures its request stream) with unique-key writes and a shared
//! register; mid-load we `kill -9` redis; then a fresh empty redis is recovered by
//! replaying the captured request stream (`ob-replay`). We then check:
//!
//!   * DURABILITY / exactly-once — every ACKNOWLEDGED write survived recovery with
//!     its exact value (0 lost, 0 torn); in-flight (ambiguous) ops are excluded
//!     (the honest output-commit gap).
//!   * LINEARIZABILITY — the shared-register history (incl. a post-recovery read)
//!     is linearizable per the from-scratch Wing-Gong oracle.
//!
//! Usage: ob-app-jepsen [--so <libobpreload.so>] [--replay <ob-replay>] [--port N]
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use onebarrier::linearizability::{is_linearizable, Action, LinOp};

fn arg(name: &str, default: &str) -> String {
    let a: Vec<String> = std::env::args().collect();
    for i in 0..a.len() {
        if a[i] == name && i + 1 < a.len() {
            return a[i + 1].clone();
        }
    }
    default.to_string()
}

fn resp(s: &mut TcpStream, parts: &[&[u8]]) -> std::io::Result<Vec<u8>> {
    let mut req = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        req.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        req.extend_from_slice(p);
        req.extend_from_slice(b"\r\n");
    }
    s.write_all(&req)?;
    s.flush()?;
    let mut buf = [0u8; 256];
    let n = s.read(&mut buf)?;
    if n == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "closed"));
    }
    Ok(buf[..n].to_vec())
}

fn parse_get(reply: &[u8]) -> i64 {
    let s = String::from_utf8_lossy(reply);
    if s.starts_with("$-1") {
        return 0;
    }
    s.split("\r\n").nth(1).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn wait_up(port: u16) -> bool {
    for _ in 0..100 {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            if resp(&mut s, &[b"PING"]).map(|r| r.starts_with(b"+PONG")).unwrap_or(false) {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn start_redis(port: u16, so: &str, cap: Option<&str>, vclock: Option<&str>) -> Child {
    let mut c = Command::new("redis-server");
    c.args([
        "--port", &port.to_string(),
        "--save", "",
        "--appendonly", "no",
        "--logfile", &format!("/tmp/ob-aj-{}.log", port),
    ]);
    c.env("LD_PRELOAD", so);
    if let Some(p) = cap { c.env("OB_CAPTURE", p); }
    if let Some(p) = vclock { c.env("OB_VCLOCK", p); }
    c.stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn().expect("spawn redis-server")
}

fn kill_port(port: u16) {
    let _ = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "for p in $(ss -tlnp 2>/dev/null|grep ':{} '|grep -oP 'pid=\\K[0-9]+'); do kill -9 $p; done",
            port
        ))
        .status();
}

fn main() {
    let so = arg("--so", "interpose/libobpreload.so");
    let replay = arg("--replay", "target/release/ob-replay");
    let port: u16 = arg("--port", "6650").parse().unwrap();
    let cap = format!("/tmp/ob-aj-capture-{}.bin", port);
    let vclock = format!("/tmp/ob-aj-vclock-{}", port);
    let _ = std::fs::remove_file(&cap);
    let _ = std::fs::remove_file(&vclock);

    println!("ob-app-jepsen — correctness torture test on UNMODIFIED redis via the libOS\n");

    kill_port(port);
    thread::sleep(Duration::from_millis(500));
    let mut child = start_redis(port, &so, Some(&cap), Some(&vclock));
    if !wait_up(port) {
        eprintln!("redis did not come up under the shim");
        let _ = child.kill();
        std::process::exit(2);
    }

    let nclients = 8u32;
    let t0 = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let acked = Arc::new(AtomicU64::new(0));
    // acknowledged unique-key writes: key -> value
    let writes: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
    // shared-register linearizability history — bounded: the Wing-Gong oracle is
    // NP-hard, so we record only a small concurrent window (like ob-lincheck's 36).
    let hist: Arc<Mutex<Vec<LinOp>>> = Arc::new(Mutex::new(Vec::new()));
    let reg_ops = Arc::new(AtomicU64::new(0));
    const REG_CAP: u64 = 32;
    const REG: &[u8] = b"reg";

    let mut handles = Vec::new();
    for cid in 0..nclients {
        let stop = stop.clone();
        let acked = acked.clone();
        let writes = writes.clone();
        let hist = hist.clone();
        let reg_ops = reg_ops.clone();
        handles.push(thread::spawn(move || {
            let now = || t0.elapsed().as_nanos() as u64;
            let mut s = match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = s.set_nodelay(true);
            let mut seq = 0u64;
            while !stop.load(Ordering::Relaxed) {
                seq += 1;
                // 1) unique-key durable write
                let key = format!("w:{}:{}", cid, seq);
                let val = (cid as i64) * 1_000_000 + seq as i64;
                match resp(&mut s, &[b"SET", key.as_bytes(), val.to_string().as_bytes()]) {
                    Ok(r) if r.starts_with(b"+OK") => {
                        writes.lock().unwrap().insert(key, val);
                        acked.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) => {}
                    Err(_) => break, // crash: in-flight op is ambiguous (excluded)
                }
                // 2) shared register: a bounded concurrent window of SET/GET, timed,
                //    for the (NP-hard) linearizability oracle.
                if reg_ops.load(Ordering::Relaxed) < REG_CAP {
                    if seq % 2 == 0 {
                        let wv = (cid as i64) * 1_000_000 + seq as i64;
                        let inv = now();
                        match resp(&mut s, &[b"SET", REG, wv.to_string().as_bytes()]) {
                            Ok(r) if r.starts_with(b"+OK") => {
                                let res = now();
                                if reg_ops.fetch_add(1, Ordering::Relaxed) < REG_CAP {
                                    hist.lock().unwrap().push(LinOp { proc_id: cid, action: Action::Write(wv), inv, res });
                                }
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    } else {
                        let inv = now();
                        match resp(&mut s, &[b"GET", REG]) {
                            Ok(r) => {
                                let res = now();
                                let v = parse_get(&r);
                                if reg_ops.fetch_add(1, Ordering::Relaxed) < REG_CAP {
                                    hist.lock().unwrap().push(LinOp { proc_id: cid, action: Action::Read(v), inv, res });
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        }));
    }

    // let the load run, then crash mid-flight
    thread::sleep(Duration::from_millis(800));
    let total_acked_before = acked.load(Ordering::Relaxed);
    println!(">>> kill -9 redis mid-load (acked so far: {})", total_acked_before);
    stop.store(true, Ordering::Relaxed);
    kill_port(port);
    let _ = child.kill();
    let _ = child.wait();
    for h in handles {
        let _ = h.join();
    }
    let cap_bytes = std::fs::metadata(&cap).map(|m| m.len()).unwrap_or(0);
    println!(">>> captured request stream: {} bytes", cap_bytes);

    // recover: fresh EMPTY redis (no libOS), replay the captured stream
    thread::sleep(Duration::from_millis(500));
    let mut fresh = start_redis(port, &so, None, None); // shim loaded but inert (no OB_CAPTURE/OB_VCLOCK)
    if !wait_up(port) {
        eprintln!("fresh redis did not come up");
        let _ = fresh.kill();
        std::process::exit(2);
    }
    let st = Command::new(&replay)
        .args(["--capture", &cap, "--target", &format!("127.0.0.1:{}", port)])
        .status()
        .expect("run ob-replay");
    println!(">>> replay-recovery exit: {}", st);

    // ---- check 1: durability / exactly-once ----
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let acked_map = writes.lock().unwrap().clone();
    let mut lost = 0u64;
    let mut torn = 0u64;
    for (k, v) in &acked_map {
        let r = resp(&mut conn, &[b"GET", k.as_bytes()]).unwrap();
        let got = parse_get(&r);
        if r.starts_with(b"$-1") {
            lost += 1;
        } else if got != *v {
            torn += 1;
        }
    }

    // ---- check 2: linearizability of the shared register (+ post-recovery read) ----
    let now = || t0.elapsed().as_nanos() as u64;
    let mut h = hist.lock().unwrap().clone();
    let inv = now();
    let r = resp(&mut conn, &[b"GET", REG]).unwrap();
    let res = now();
    h.push(LinOp { proc_id: 999, action: Action::Read(parse_get(&r)), inv, res });
    let lin = is_linearizable(&h, 0);

    kill_port(port);
    let _ = fresh.kill();

    println!("\n  acknowledged unique writes:  {}", acked_map.len());
    println!("  LOST acked writes:           {}   <- must be 0", lost);
    println!("  TORN values:                 {}   <- must be 0", torn);
    println!("  register history size:       {}", h.len());
    println!("  LINEARIZABLE:                {}", lin);

    if lost == 0 && torn == 0 && lin {
        println!("\nRESULT: PASS — unmodified redis recovered via replay is durable (every acked\n        write survived exactly) AND linearizable.");
    } else {
        println!("\nRESULT: FAIL");
        std::process::exit(1);
    }
}
