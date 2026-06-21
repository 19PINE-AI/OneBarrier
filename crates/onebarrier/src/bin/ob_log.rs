//! `ob-log` — the OneBarrier durable pub/sub streaming log (Storm/Kafka class).
//!   ob-log [--port N] [--dir PATH] [--fsync] [--snap-interval N]
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use onebarrier::server::{run, ServerConfig};
use onebarrier::streamlog::StreamLogProtocol;

fn main() -> std::io::Result<()> {
    let (mut port, mut dir, mut fsync, mut snap) = (7199u16, PathBuf::from("/tmp/ob-log-data"), false, 10_000u64);
    let mut a = std::env::args().skip(1);
    while let Some(x) = a.next() {
        match x.as_str() {
            "--port" => port = a.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--dir" => dir = a.next().map(PathBuf::from).unwrap_or(dir),
            "--fsync" => fsync = true,
            "--snap-interval" => snap = a.next().and_then(|v| v.parse().ok()).unwrap_or(snap),
            _ => {}
        }
    }
    run(ServerConfig { addr: SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port), dir, snap_interval: snap, fsync }, Arc::new(StreamLogProtocol), "ob-log")
}
