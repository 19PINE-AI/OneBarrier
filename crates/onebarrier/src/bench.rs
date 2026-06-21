//! M2 — RQ2, the make-or-break measurement (docs/PLAN.md §7).
//!
//! Thesis: OneBarrier's output-commit barrier coincides with 1Pipe's reliable
//! 2PC commit barrier, so FT's marginal cost over the reliable-fabric baseline
//! is ≈ 0 **when durability rides the fabric** (in-fabric replication / in-memory
//! log), and *stacks* only when durability needs stable storage (fsync).
//!
//! We measure, per op, on the live loopback-UDP fabric:
//!   * `delivery`  — client-send → executor-delivery (the reliable-1Pipe baseline;
//!                   includes the 2PC commit barrier, i.e. all replicas have it);
//!   * `durable`   — the MARGINAL cost OneBarrier adds at the executor: apply +
//!                   append to the durable log, with or without `fsync`;
//!   * `commit`    — `delivery + durable` = output-commit-ready latency.
//!
//! The in-fabric/in-memory tier should show `durable ≈ 0` (overlap); the fsync
//! tier should show `durable ≈ disk latency` (stack). That contrast *is* RQ2.
//!
//! Absolute numbers are the **UDP reproduction**, not the RDMA/Tofino testbed —
//! the paper measures 1–2 µs RTT; here delivery is loopback-UDP. The *shape*
//! (overlap vs stack, and marginal ≈ 0 for the in-fabric tier) is what transfers.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use onepipe_core::reliable::PeerId;
use onepipe_net::{HostConfig, ReliableHost, UdpEndpoint};

use crate::{Engine, KvStore, Op};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// Op reaches all replicas via the scatter (durability = the commit barrier);
    /// executor keeps an in-memory ordered log. Marginal cost should be ≈ 0.
    InFabricMem,
    /// Executor fsyncs the log record to stable storage before output-commit.
    Fsync,
}

#[derive(Clone, Copy, Debug)]
pub struct BenchConfig {
    pub clients: usize,
    pub ops_per_client: u64,
    pub keys: u64,
    /// Pacing between a client's ops — keep load low for idle latency (the
    /// paper's Fig 9a regime: zero queuing delay).
    pub pace: Duration,
    pub durability: Durability,
    pub host: HostConfig,
    pub timeout: Duration,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            clients: 2,
            ops_per_client: 2000,
            keys: 64,
            pace: Duration::from_micros(300),
            durability: Durability::InFabricMem,
            host: HostConfig {
                beacon_interval: Duration::from_micros(500),
                retx_interval: Duration::from_millis(5),
                failure_timeout: Duration::from_millis(1500),
                // On a single loopback machine every thread reads the same system
                // wall clock, so send/deliver timestamps are directly comparable;
                // clock-sync would only add an offset that corrupts the latency
                // delta. Disable it for the measurement.
                clock_sync: false,
                ..HostConfig::default()
            },
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BenchResult {
    pub durability: String,
    pub n: usize,
    pub delivery_p50: u64,
    pub delivery_p99: u64,
    pub durable_p50: u64,
    pub durable_p99: u64,
    pub commit_p50: u64,
    pub commit_p99: u64,
}

fn loopback() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
}

fn now_ns() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

fn pct(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Run one tier and return its latency percentiles. Topology: executor = id 0,
/// clients = ids 1..=clients, all on loopback UDP.
pub fn run_bench(cfg: &BenchConfig, dir: &std::path::Path) -> io::Result<BenchResult> {
    let n = 1 + cfg.clients;
    let eps: Vec<UdpEndpoint> = (0..n).map(|_| UdpEndpoint::bind(loopback())).collect::<io::Result<_>>()?;
    let addrs: Vec<SocketAddr> = eps.iter().map(UdpEndpoint::local_addr).collect::<io::Result<_>>()?;
    let total = cfg.ops_per_client * cfg.clients as u64;
    let done = Arc::new(AtomicBool::new(false));
    let fsync = cfg.durability == Durability::Fsync;
    let mut handles = Vec::new();

    for (i, ep) in eps.into_iter().enumerate() {
        let me = i as PeerId;
        let peers: Vec<(PeerId, SocketAddr)> =
            (0..n).filter(|&j| j != i).map(|j| (j as PeerId, addrs[j])).collect();
        let cfg = *cfg;
        let done = Arc::clone(&done);
        let dir = dir.to_path_buf();

        handles.push(thread::spawn(move || -> io::Result<Vec<(u64, u64)>> {
            let mut host = ReliableHost::new(ep, me, &peers, cfg.host);
            let started = Instant::now();

            if me == 0 {
                // Executor: apply the totally-ordered stream, measure marginal
                // durability cost, collect (delivery_ns, durable_ns) samples.
                let mut eng =
                    Engine::<KvStore>::create(dir.join("executor"), 1_000_000, fsync)?;
                let mut samples: Vec<(u64, u64)> = Vec::with_capacity(total as usize);
                let mut applied = 0u64;
                loop {
                    for d in host.poll(Duration::from_micros(200)).unwrap_or_default() {
                        // 1Pipe timestamps are 48-bit-masked wall-clock ns; mask
                        // `now` the same way and subtract modulo 2^48 so the delta
                        // is correct (elapsed ≪ 2^48 ns ≈ 78 h).
                        const MASK48: u64 = (1 << 48) - 1;
                        let delivery = (now_ns() & MASK48).wrapping_sub(d.msg_ts.as_nanos()) & MASK48;
                        if let Some(op) = Op::decode(&d.payload) {
                            let t0 = Instant::now();
                            let out = eng.deliver(d.msg_ts, &op)?;
                            let durable = t0.elapsed().as_nanos() as u64;
                            if !matches!(out, crate::Output::Suppressed) {
                                samples.push((delivery, durable));
                                applied += 1;
                            }
                        }
                    }
                    if applied >= total {
                        done.store(true, Ordering::SeqCst);
                        break;
                    }
                    if started.elapsed() >= cfg.timeout {
                        break;
                    }
                }
                Ok(samples)
            } else {
                // Client: paced INCR ops to the executor (low load = idle latency).
                let c = me as u32;
                for i in 0..cfg.ops_per_client {
                    let op = Op::incr(c, i + 1, &format!("k{}", i % cfg.keys), 1);
                    host.send(&[0], &op.encode());
                    let t = Instant::now() + cfg.pace;
                    while Instant::now() < t {
                        let _ = host.poll(Duration::from_micros(50));
                    }
                }
                while !done.load(Ordering::SeqCst) && started.elapsed() < cfg.timeout {
                    let _ = host.poll(Duration::from_millis(1));
                }
                Ok(Vec::new())
            }
        }));
    }

    let mut samples: Vec<(u64, u64)> = Vec::new();
    for h in handles {
        samples.extend(h.join().unwrap()?);
    }

    let mut delivery: Vec<u64> = samples.iter().map(|(d, _)| *d).collect();
    let mut durable: Vec<u64> = samples.iter().map(|(_, d)| *d).collect();
    let mut commit: Vec<u64> = samples.iter().map(|(a, b)| a + b).collect();
    delivery.sort_unstable();
    durable.sort_unstable();
    commit.sort_unstable();

    Ok(BenchResult {
        durability: format!("{:?}", cfg.durability),
        n: samples.len(),
        delivery_p50: pct(&delivery, 0.50),
        delivery_p99: pct(&delivery, 0.99),
        durable_p50: pct(&durable, 0.50),
        durable_p99: pct(&durable, 0.99),
        commit_p50: pct(&commit, 0.50),
        commit_p99: pct(&commit, 0.99),
    })
}

// ---------------------------------------------------------------------------
// RQ8 — snapshot-interval tradeoff (engine level, deterministic)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SnapResult {
    pub interval: u64,
    pub ops: u64,
    /// Snapshots taken during the run (steady-state overhead driver).
    pub snapshots: u64,
    /// Per-op apply cost incl. amortized snapshotting (µs).
    pub apply_us_per_op: f64,
    /// Records replayed on recovery (≤ interval): the recovery-cost driver.
    pub replay_records: u64,
    /// Wall-clock recovery time (µs).
    pub recover_us: f64,
}

/// Sweep the snapshot interval and measure the steady-state-overhead vs
/// recovery-cost tradeoff (docs/PLAN.md §7 RQ8; the recovery-model `I*` rule):
/// small interval ⇒ more snapshots (higher steady overhead), fewer replay
/// records (faster recovery); large interval ⇒ the reverse.
pub fn sweep_snapshot_interval(dir: &std::path::Path, intervals: &[u64], ops: u64, keys: u64) -> Vec<SnapResult> {
    let mut out = Vec::new();
    for &interval in intervals {
        let d = dir.join(format!("snap-{interval}"));
        let _ = std::fs::remove_dir_all(&d);

        let t0 = Instant::now();
        let mut e = crate::Engine::<crate::KvStore>::create(&d, interval, false).unwrap();
        for i in 0..ops {
            let op = crate::Op::incr(1, i + 1, &format!("k{}", i % keys), 1);
            e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
        }
        let apply_us = t0.elapsed().as_nanos() as f64 / 1000.0;
        let snapshots = e.stats.snapshots;
        drop(e); // close durable store

        let t1 = Instant::now();
        let er = crate::Engine::<crate::KvStore>::recover(&d, interval, false).unwrap();
        let recover_us = t1.elapsed().as_nanos() as f64 / 1000.0;

        out.push(SnapResult {
            interval,
            ops,
            snapshots,
            apply_us_per_op: apply_us / ops as f64,
            replay_records: er.stats.replayed,
            recover_us,
        });
        let _ = std::fs::remove_dir_all(&d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let k = CTR.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("ob-bench-{}-{}-{}", tag, std::process::id(), k));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn snapshot_interval_tradeoff_holds() {
        // Smaller interval ⇒ more snapshots but fewer replay records on recovery.
        let dir = tmpdir("snapsweep");
        let r = sweep_snapshot_interval(&dir, &[64, 100_000], 20_000, 32);
        let small = &r[0];
        let large = &r[1];
        assert!(small.snapshots > large.snapshots, "small interval should snapshot more: {} vs {}", small.snapshots, large.snapshots);
        assert!(small.replay_records < large.replay_records, "small interval should replay fewer: {} vs {}", small.replay_records, large.replay_records);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn in_fabric_durability_is_near_free_vs_fsync() {
        // RQ2 core assertion: the in-fabric/in-memory durability marginal cost is
        // far below the fsync marginal cost — the overlap-vs-stack contrast.
        let small = BenchConfig {
            clients: 2,
            ops_per_client: 400,
            pace: Duration::from_micros(200),
            ..Default::default()
        };
        let dir_mem = tmpdir("mem");
        let mem = run_bench(&BenchConfig { durability: Durability::InFabricMem, ..small }, &dir_mem).unwrap();
        let dir_fs = tmpdir("fsync");
        let fs = run_bench(&BenchConfig { durability: Durability::Fsync, ..small }, &dir_fs).unwrap();

        assert!(mem.n > 700 && fs.n > 700, "too few samples: mem={} fs={}", mem.n, fs.n);
        // In-fabric/in-memory marginal durability is sub-microsecond-ish and must
        // be well below fsync's. (Absolute values are reproduction, not RDMA.)
        assert!(
            mem.durable_p50 < fs.durable_p50,
            "expected in-fabric mem durable ({} ns) < fsync durable ({} ns)",
            mem.durable_p50, fs.durable_p50
        );
        let _ = std::fs::remove_dir_all(&dir_mem);
        let _ = std::fs::remove_dir_all(&dir_fs);
    }
}
