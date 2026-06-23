//! `ob-replay` — rebuild an unmodified server's state from a OneBarrier capture
//! log (produced by the `obpreload` LD_PRELOAD shim). Groups the captured
//! inbound request bytes by connection and replays each connection's stream
//! against a fresh server instance — transparent record-replay recovery for a
//! binary that knows nothing about OneBarrier.
//!
//!   ob-replay --capture /tmp/cap.log --target 127.0.0.1:6390

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let mut capture = String::from("/tmp/ob-capture.log");
    let mut target = String::from("127.0.0.1:6390");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--capture" => capture = args.next().unwrap_or(capture),
            "--target" => target = args.next().unwrap_or(target),
            _ => {}
        }
    }

    let raw = std::fs::read(&capture)?;
    // Records: [u32 conn_id][u32 len][len bytes], in capture (arrival) order.
    // Preserve per-connection byte order; keep connections in first-seen order.
    let mut conns: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut order: Vec<u32> = Vec::new();
    let mut p = 0usize;
    while p + 8 <= raw.len() {
        let cid = u32::from_le_bytes(raw[p..p + 4].try_into().unwrap());
        let len = u32::from_le_bytes(raw[p + 4..p + 8].try_into().unwrap()) as usize;
        p += 8;
        if p + len > raw.len() {
            break;
        }
        if !conns.contains_key(&cid) {
            order.push(cid);
        }
        conns.entry(cid).or_default().extend_from_slice(&raw[p..p + len]);
        p += len;
    }

    // Replay every connection's stream in PARALLEL (one thread per connection) so
    // recovery time is bounded by the slowest single connection, not the sum — and
    // drain replies with a short idle timeout instead of a long fixed wait.
    let handles: Vec<_> = order
        .iter()
        .map(|cid| {
            let bytes = conns[cid].clone();
            let target = target.clone();
            std::thread::spawn(move || -> std::io::Result<usize> {
                let mut s = TcpStream::connect(&target)?;
                s.set_nodelay(true).ok();
                s.write_all(&bytes)?;
                s.flush()?;
                s.set_read_timeout(Some(Duration::from_millis(25))).ok();
                let mut buf = [0u8; 65536];
                loop {
                    match s.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(_) => break, // short idle: replies drained
                    }
                }
                Ok(bytes.len())
            })
        })
        .collect();
    let mut total_bytes = 0usize;
    for h in handles {
        total_bytes += h.join().unwrap()?;
    }

    println!(
        "ob-replay: replayed {} connection(s), {} request bytes from {} to {}",
        order.len(),
        total_bytes,
        capture,
        target
    );
    Ok(())
}
