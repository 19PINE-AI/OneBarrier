//! `ob-mc` — the OneBarrier Memcached (text-protocol) server. Durable,
//! crash-recoverable. Drive with `memtier_benchmark --protocol=memcache_text -p
//! <port>` or `nc <host> <port>`.
//!
//!   ob-mc [--port N] [--dir PATH] [--fsync] [--snap-interval N]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use onebarrier::memcache::MemcacheProtocol;
use onebarrier::server::{run, ServerConfig};

fn main() -> std::io::Result<()> {
    let mut port: u16 = 11311;
    let mut dir = PathBuf::from("/tmp/ob-mc-data");
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
                println!("ob-mc [--port N] [--dir PATH] [--fsync] [--snap-interval N]");
                return Ok(());
            }
            _ => {}
        }
    }

    run(
        ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port),
            dir,
            snap_interval,
            fsync,
        },
        Arc::new(MemcacheProtocol),
        "ob-mc",
    )
}
