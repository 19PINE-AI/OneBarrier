//! `ob-lincheck` — paper exp #7: drive the real `ob-kv` server with concurrent
//! clients on one register (SET=Write, GET=Read), record the real-time history,
//! and run the from-scratch Wing-Gong linearizability checker. A pass is a real
//! linearizability verdict (not the acked-set heuristic of `ob-jepsen`).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use onebarrier::linearizability::{is_linearizable, Action, LinOp};
use onebarrier::server::{start_resp, ServerConfig};

fn resp_cmd(s: &mut TcpStream, parts: &[&[u8]]) -> Vec<u8> {
    let mut req = format!("*{}\r\n", parts.len()).into_bytes();
    for p in parts {
        req.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        req.extend_from_slice(p);
        req.extend_from_slice(b"\r\n");
    }
    s.write_all(&req).unwrap();
    s.flush().unwrap();
    let mut buf = [0u8; 128];
    let n = s.read(&mut buf).unwrap();
    buf[..n].to_vec()
}

fn parse_get(reply: &[u8]) -> i64 {
    // $<len>\r\n<val>\r\n  or  $-1\r\n (nil → 0)
    let s = String::from_utf8_lossy(reply);
    if s.starts_with("$-1") {
        return 0;
    }
    s.split("\r\n").nth(1).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn main() {
    let dir = std::env::temp_dir().join(format!("ob-lincheck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let h = start_resp(ServerConfig {
        addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
        dir: dir.clone(),
        snap_interval: 1000,
        fsync: false,
    })
    .unwrap();
    let addr = h.addr;

    // Initialize the register.
    {
        let mut s = TcpStream::connect(addr).unwrap();
        resp_cmd(&mut s, &[b"SET", b"reg", b"0"]);
    }

    let clients = 4usize;
    let ops_per_client = 9u64;
    let start = Instant::now();
    let history: Arc<Mutex<Vec<LinOp>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<_> = (0..clients)
        .map(|c| {
            let history = Arc::clone(&history);
            thread::spawn(move || {
                let mut s = TcpStream::connect(addr).unwrap();
                for i in 0..ops_per_client {
                    let now = || start.elapsed().as_nanos() as u64;
                    let (action, inv, res) = if i % 2 == 0 {
                        // Write a unique value.
                        let v = (c as i64 + 1) * 100 + i as i64;
                        let inv = now();
                        resp_cmd(&mut s, &[b"SET", b"reg", v.to_string().as_bytes()]);
                        (Action::Write(v), inv, now())
                    } else {
                        let inv = now();
                        let reply = resp_cmd(&mut s, &[b"GET", b"reg"]);
                        (Action::Read(parse_get(&reply)), inv, now())
                    };
                    history.lock().unwrap().push(LinOp { proc_id: c as u32, action, inv, res });
                }
            })
        })
        .collect();
    for hd in handles {
        hd.join().unwrap();
    }
    h.stop();
    let _ = std::fs::remove_dir_all(&dir);

    let hist = history.lock().unwrap();
    println!("OneBarrier — real linearizability check (paper exp #7)");
    println!("  {clients} concurrent clients, {} ops on one register, recorded with real-time intervals\n", hist.len());
    let ok = is_linearizable(&hist, 0);
    println!("  history size: {}", hist.len());
    println!("  LINEARIZABLE: {ok}");
    println!("\n{}", if ok {
        "PASS — the real concurrent OneBarrier history is linearizable (a from-scratch\n\
         Wing-Gong oracle confirms it, not just an acked-set heuristic)."
    } else {
        "FAIL — linearizability violation found."
    });
    std::process::exit(i32::from(!ok));
}
