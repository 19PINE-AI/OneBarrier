//! `ob-kv` — the OneBarrier RESP key-value server. A durable, crash-recoverable
//! Redis-protocol service on the OneBarrier engine. Drive it with `redis-cli -p
//! <port>` or `redis-benchmark -p <port> -t set,get,incr`.
//!
//!   ob-kv [--port N] [--dir PATH] [--fsync] [--snap-interval N]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use onebarrier::server::{run, ServerConfig};

fn main() -> std::io::Result<()> {
    let mut port: u16 = 6399;
    let mut dir = PathBuf::from("/tmp/ob-kv-data");
    let mut fsync = false;
    let mut snap_interval: u64 = 10_000;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--dir" => dir = args.next().map(PathBuf::from).unwrap_or(dir),
            "--fsync" => fsync = true,
            "--snap-interval" => {
                snap_interval = args.next().and_then(|v| v.parse().ok()).unwrap_or(snap_interval);
            }
            "--help" | "-h" => {
                println!("ob-kv [--port N] [--dir PATH] [--fsync] [--snap-interval N]");
                return Ok(());
            }
            _ => {}
        }
    }

    run(ServerConfig {
        addr: SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port),
        dir,
        snap_interval,
        fsync,
    })
}
