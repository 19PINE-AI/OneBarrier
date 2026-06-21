//! `ob-http` — the OneBarrier HTTP/1.1 REST key-value server. Durable,
//! crash-recoverable. Drive with `curl` or benchmark with `ab`/`wrk`:
//!
//!   curl -XPUT --data hello localhost:8088/greeting
//!   curl localhost:8088/greeting
//!   curl -XPOST localhost:8088/incr/views
//!
//!   ob-http [--port N] [--dir PATH] [--fsync] [--snap-interval N]

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use onebarrier::http::HttpProtocol;
use onebarrier::server::{run, ServerConfig};

fn main() -> std::io::Result<()> {
    let mut port: u16 = 8088;
    let mut dir = PathBuf::from("/tmp/ob-http-data");
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
                println!("ob-http [--port N] [--dir PATH] [--fsync] [--snap-interval N]");
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
        Arc::new(HttpProtocol),
        "ob-http",
    )
}
