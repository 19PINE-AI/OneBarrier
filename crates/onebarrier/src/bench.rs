//! M2 — RQ2, the make-or-break measurement (docs/research/PLAN.md §7).
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
/// recovery-cost tradeoff (docs/research/PLAN.md §7 RQ8; the recovery-model `I*` rule):
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

// ---------------------------------------------------------------------------
// RQ5 — passive (OneBarrier) vs active SMR execution CPU
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct CpuResult {
    pub mode: String,
    pub replicas: usize,
    /// Total execution CPU across all replicas (ms) — the resource the FT scheme
    /// spends on running the state machine.
    pub exec_cpu_ms: f64,
}

/// Busy-spin `us` microseconds to model an expensive state-machine apply.
fn spin_us(us: u64) {
    if us == 0 {
        return;
    }
    let end = Instant::now() + Duration::from_micros(us);
    while Instant::now() < end {
        std::hint::spin_loop();
    }
}

/// RQ5: compare execution CPU of **active SMR** (every one of `replicas` runs the
/// state machine) vs **OneBarrier passive** (one executor runs it; the other
/// `replicas-1` are log-only backups that durably store ops without executing).
/// With a non-trivial apply cost, passive spends ≈ 1/replicas the execution CPU.
pub fn bench_cpu_passive_vs_active(dir: &std::path::Path, ops: u64, apply_us: u64, replicas: usize) -> (CpuResult, CpuResult) {
    use std::thread;

    // Active: `replicas` executor threads, each applies every op (+apply cost).
    let active_cpu = {
        let handles: Vec<_> = (0..replicas)
            .map(|r| {
                let d = dir.join(format!("active-{r}"));
                let _ = std::fs::remove_dir_all(&d);
                thread::spawn(move || {
                    let mut e = crate::Engine::<crate::KvStore>::create(&d, 1_000_000, false).unwrap();
                    let t = Instant::now();
                    for i in 0..ops {
                        let op = crate::Op::incr(1, i + 1, &format!("k{}", i % 64), 1);
                        e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
                        spin_us(apply_us);
                    }
                    t.elapsed().as_secs_f64() * 1000.0
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum::<f64>()
    };

    // Passive: 1 executor (applies + apply cost) + (replicas-1) log-only backups.
    let passive_cpu = {
        let exec = {
            let d = dir.join("passive-exec");
            let _ = std::fs::remove_dir_all(&d);
            thread::spawn(move || {
                let mut e = crate::Engine::<crate::KvStore>::create(&d, 1_000_000, false).unwrap();
                let t = Instant::now();
                for i in 0..ops {
                    let op = crate::Op::incr(1, i + 1, &format!("k{}", i % 64), 1);
                    e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
                    spin_us(apply_us);
                }
                t.elapsed().as_secs_f64() * 1000.0
            })
        };
        let backups: Vec<_> = (0..replicas.saturating_sub(1))
            .map(|r| {
                let d = dir.join(format!("passive-log-{r}"));
                let _ = std::fs::remove_dir_all(&d);
                thread::spawn(move || {
                    // Log-only: durably store each op, do NOT execute the state machine.
                    let mut log = crate::durable::Durable::open(&d, false).unwrap();
                    let t = Instant::now();
                    for i in 0..ops {
                        let op = crate::Op::incr(1, i + 1, &format!("k{}", i % 64), 1);
                        log.append(i + 1, &op.encode()).unwrap();
                    }
                    t.elapsed().as_secs_f64() * 1000.0
                })
            })
            .collect();
        exec.join().unwrap() + backups.into_iter().map(|h| h.join().unwrap()).sum::<f64>()
    };

    let _ = std::fs::remove_dir_all(dir);
    (
        CpuResult { mode: "active-SMR".into(), replicas, exec_cpu_ms: active_cpu },
        CpuResult { mode: "passive-OneBarrier".into(), replicas, exec_cpu_ms: passive_cpu },
    )
}

// ---------------------------------------------------------------------------
// RQ7 — establishing total order: central sequencer vs fabric/timestamp
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct OrderResult {
    pub mode: String,
    pub threads: usize,
    pub ops_per_sec: f64,
}

/// RQ7 (contribution isolation): the cost of *establishing* a replayable total
/// order. The LLFT/NOPaxos-style **central sequencer** forces every op through a
/// single serialization point (all producers contend); the **fabric/timestamp**
/// approach (1Pipe / OneBarrier) assigns order without shared contention and
/// reconstructs the total order by timestamp. With this software model, the
/// sequencer bottlenecks as producers grow while the timestamp approach scales —
/// the reason OneBarrier inherits no central-sequencer cost (cf. 1Pipe paper
/// Fig 8, measured 2–20× on hardware). This is a model of the *ordering
/// mechanism*, not the full system.
pub fn bench_ordering(threads: usize, ops_per_thread: u64) -> (OrderResult, OrderResult) {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let total = (threads as u64 * ops_per_thread) as f64;

    // Central sequencer: every op acquires the one global counter.
    let seq_ops_per_sec = {
        let seq = Arc::new(Mutex::new(0u64));
        let t0 = Instant::now();
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let seq = Arc::clone(&seq);
                thread::spawn(move || {
                    for _ in 0..ops_per_thread {
                        let mut g = seq.lock().unwrap();
                        *g += 1; // the serialization point
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        total / t0.elapsed().as_secs_f64()
    };

    // Fabric/timestamp: each producer assigns order locally (no shared lock).
    let ts_ops_per_sec = {
        let t0 = Instant::now();
        let handles: Vec<_> = (0..threads)
            .map(|tid| {
                thread::spawn(move || {
                    let mut local = (tid as u64) << 40; // per-source monotonic base
                    let mut sink = 0u64;
                    for _ in 0..ops_per_thread {
                        local += 1; // assign a timestamp, no contention
                        sink ^= local;
                    }
                    sink
                })
            })
            .collect();
        for h in handles {
            let _ = h.join().unwrap();
        }
        total / t0.elapsed().as_secs_f64()
    };

    (
        OrderResult { mode: "central-sequencer".into(), threads, ops_per_sec: seq_ops_per_sec },
        OrderResult { mode: "fabric/timestamp".into(), threads, ops_per_sec: ts_ops_per_sec },
    )
}

// ---------------------------------------------------------------------------
// M4 — FT order-establishment baselines: OneBarrier vs LLFT vs HyCoR
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct FtBaseline {
    pub mode: String,
    pub threads: usize,
    pub ops_per_sec: f64,
}

/// M4 head-to-head: how each transparent-FT approach establishes a *replayable*
/// total order, and what it costs per op under `threads` concurrent producers.
///   * **OneBarrier** — the fabric supplies the order; the replica only applies +
///     appends the op (no per-op ordering metadata, no sequencer).
///   * **LLFT** — a host-level virtual-time *sequencer* assigns the order; every
///     op serializes through it (the cost the in-network fabric removes).
///   * **HyCoR** — logs the receipt *non-determinism* per op (an extra ordering
///     record) so replay can reconstruct the order, plus applies + appends.
/// Same apply + op-append work in all three; the delta is purely the ordering
/// mechanism. (Software model on the reproduction; the real fabric advantage is
/// 1Pipe's measured in-network result.)
pub fn bench_ft_baselines(dir: &std::path::Path, threads: usize, ops_per_thread: u64) -> Vec<FtBaseline> {
    use std::sync::{Arc, Mutex};
    use std::thread;
    let total = (threads as u64 * ops_per_thread) as f64;

    let run_per_thread = |label: &str, body: Arc<dyn Fn(usize) + Send + Sync>| -> FtBaseline {
        let t0 = Instant::now();
        let handles: Vec<_> = (0..threads)
            .map(|tid| {
                let body = Arc::clone(&body);
                thread::spawn(move || body(tid))
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        FtBaseline { mode: label.into(), threads, ops_per_sec: total / t0.elapsed().as_secs_f64() }
    };

    let base = dir.to_path_buf();

    // OneBarrier: apply + append op (order is the fabric's; nothing extra).
    let ob = {
        let base = base.clone();
        run_per_thread("OneBarrier (fabric order)", Arc::new(move |tid: usize| {
            let d = base.join(format!("ob-{tid}"));
            let _ = std::fs::remove_dir_all(&d);
            let mut e = crate::Engine::<crate::KvStore>::create(&d, 1_000_000, false).unwrap();
            for i in 0..ops_per_thread {
                let op = crate::Op::incr(1, i + 1, "k", 1);
                e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
            }
        }))
    };

    // LLFT: every op serializes through one host virtual-time sequencer.
    let llft = {
        let base = base.clone();
        let seq = Arc::new(Mutex::new(0u64));
        run_per_thread("LLFT (host sequencer)", Arc::new(move |tid: usize| {
            let d = base.join(format!("llft-{tid}"));
            let _ = std::fs::remove_dir_all(&d);
            let mut e = crate::Engine::<crate::KvStore>::create(&d, 1_000_000, false).unwrap();
            for i in 0..ops_per_thread {
                {
                    let mut g = seq.lock().unwrap();
                    *g += 1; // serialization point: assign global virtual time
                }
                let op = crate::Op::incr(1, i + 1, "k", 1);
                e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
            }
        }))
    };

    // HyCoR: apply + append op + append a separate per-op order/nondeterminism record.
    let hycor = {
        let base = base.clone();
        run_per_thread("HyCoR (nondeterminism log)", Arc::new(move |tid: usize| {
            let d = base.join(format!("hycor-{tid}"));
            let _ = std::fs::remove_dir_all(&d);
            let mut e = crate::Engine::<crate::KvStore>::create(&d, 1_000_000, false).unwrap();
            let mut orderlog = crate::durable::Durable::open(base.join(format!("hycor-ord-{tid}")), false).unwrap();
            for i in 0..ops_per_thread {
                let op = crate::Op::incr(1, i + 1, "k", 1);
                e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &op).unwrap();
                // The order-log OneBarrier does NOT keep: per-op receipt order.
                orderlog.append(i + 1, &(i + 1).to_le_bytes()).unwrap();
            }
        }))
    };

    let _ = std::fs::remove_dir_all(dir);
    vec![ob, hycor, llft]
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
    fn onebarrier_writes_no_order_log_unlike_hycor() {
        // Deterministic structural result (throughput on a shared host is noisy):
        // OneBarrier persists only the op-log; HyCoR additionally persists a
        // per-op order/non-determinism record, so it writes strictly more durable
        // bytes for the same workload. That extra log is the overhead the fabric's
        // total order removes.
        let dir = tmpdir("ftbytes");
        let ops = 5_000u64;

        let ob_dir = dir.join("ob");
        let mut e = crate::Engine::<crate::KvStore>::create(&ob_dir, 1_000_000, false).unwrap();
        for i in 0..ops {
            e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &crate::Op::incr(1, i + 1, "k", 1)).unwrap();
        }
        drop(e);
        let ob_bytes = std::fs::metadata(ob_dir.join("oplog")).map(|m| m.len()).unwrap_or(0);

        let hy_dir = dir.join("hy");
        let mut e = crate::Engine::<crate::KvStore>::create(&hy_dir, 1_000_000, false).unwrap();
        let mut orderlog = crate::durable::Durable::open(hy_dir.join("ord"), false).unwrap();
        for i in 0..ops {
            e.deliver(onepipe_core::timestamp::Timestamp::from_nanos(i + 1), &crate::Op::incr(1, i + 1, "k", 1)).unwrap();
            orderlog.append(i + 1, &(i + 1).to_le_bytes()).unwrap();
        }
        drop(e);
        let hy_op = std::fs::metadata(hy_dir.join("oplog")).map(|m| m.len()).unwrap_or(0);
        let hy_ord = std::fs::metadata(hy_dir.join("ord").join("oplog")).map(|m| m.len()).unwrap_or(0);

        assert_eq!(ob_bytes, hy_op, "same op-log");
        assert!(hy_ord > 0, "HyCoR keeps a non-empty order-log");
        assert!(hy_op + hy_ord > ob_bytes, "HyCoR writes strictly more durable bytes than OneBarrier");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fabric_ordering_scales_past_central_sequencer() {
        // At 8 producers, lock-free timestamp ordering beats the contended
        // central sequencer (the LLFT/NOPaxos serialization point).
        let (seq, ts) = bench_ordering(8, 200_000);
        assert!(
            ts.ops_per_sec > seq.ops_per_sec,
            "timestamp {} ops/s should beat sequencer {} ops/s",
            ts.ops_per_sec, seq.ops_per_sec
        );
    }

    #[test]
    fn passive_uses_less_execution_cpu_than_active_smr() {
        // With a real apply cost, passive (1 executor + log-only backups) spends
        // markedly less execution CPU than active SMR (N executors).
        let dir = tmpdir("cpu");
        let (active, passive) = bench_cpu_passive_vs_active(&dir, 2_000, 20, 4);
        assert!(
            passive.exec_cpu_ms < active.exec_cpu_ms * 0.6,
            "passive {} ms should be well under active {} ms",
            passive.exec_cpu_ms, active.exec_cpu_ms
        );
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
