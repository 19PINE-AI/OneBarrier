//! M1 — a runnable OneBarrier cluster on the **live** 1Pipe fabric (loopback
//! UDP, the same `ReliableHost` path the 1Pipe integration tests exercise).
//!
//! Topology: `clients` sender nodes scatter ops to `replicas` executor nodes.
//! Every replica applies the fabric's *single global total order* through its
//! own [`Engine`]. Because the order is total and execution is deterministic,
//! **all replicas must converge to identical state** — and for the
//! commutative INCR workload, to an exactly-predictable state (so we check
//! correctness, not just agreement). This is deterministic replicated execution
//! over the real fabric, with no message-order log (the fabric is the order).
//!
//! Recovery-over-fabric (a killed replica catching up from replicated log) is
//! M2; single-node durable recovery is already proven in the `lib` unit tests.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use onepipe_core::reliable::PeerId;
use onepipe_net::{HostConfig, ReliableHost, UdpEndpoint};

use crate::{Engine, KvStore, Op};

#[derive(Clone, Copy, Debug)]
pub struct ClusterConfig {
    pub replicas: usize,
    pub clients: usize,
    pub ops_per_client: u64,
    /// Keyspace the INCR workload spreads over (each op +1 to one key).
    pub keys: u64,
    pub snap_interval: u64,
    pub host: HostConfig,
    pub timeout: Duration,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            replicas: 3,
            clients: 2,
            ops_per_client: 200,
            keys: 8,
            snap_interval: 64,
            host: HostConfig {
                beacon_interval: Duration::from_millis(1),
                retx_interval: Duration::from_millis(5),
                failure_timeout: Duration::from_millis(1500),
                ..HostConfig::default()
            },
            timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClusterReport {
    /// Sorted KV contents of each replica (by replica id).
    pub replica_states: Vec<(PeerId, Vec<(String, i64)>)>,
    /// The state every replica should hold (commutative INCR sums).
    pub expected: Vec<(String, i64)>,
    /// All replicas hold identical state.
    pub converged: bool,
    /// That identical state equals `expected` (exactly-once, nothing lost/dup'd).
    pub correct: bool,
    pub timed_out: bool,
}

fn loopback() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
}

/// The deterministic INCR workload for client `c`: `ops` increments of +1,
/// round-robin over `keys`. `seq` is per-client and starts at 1.
fn client_ops(c: u32, ops: u64, keys: u64) -> Vec<Op> {
    (0..ops)
        .map(|i| Op::incr(c, i + 1, &format!("k{}", i % keys), 1))
        .collect()
}

/// Expected final state: every key gets (total ops over that key) increments.
fn expected_state(clients: usize, ops: u64, keys: u64) -> Vec<(String, i64)> {
    let mut v = Vec::new();
    for k in 0..keys {
        let mut sum = 0i64;
        for c in 0..clients as u64 {
            // client c emits op i (i in 0..ops) onto key (i % keys)
            sum += (0..ops).filter(|i| i % keys == k).count() as i64;
            let _ = c;
        }
        v.push((format!("k{k}"), sum));
    }
    v.sort();
    v
}

/// Run a OneBarrier cluster on loopback and report convergence + correctness.
/// `dir` roots each replica's durable store (`dir/replica-<id>`).
pub fn run_cluster(cfg: &ClusterConfig, dir: &std::path::Path) -> io::Result<ClusterReport> {
    let n = cfg.replicas + cfg.clients;
    // Replicas get ids [0, replicas); clients get [replicas, n).
    let eps: Vec<UdpEndpoint> = (0..n)
        .map(|_| UdpEndpoint::bind(loopback()))
        .collect::<io::Result<_>>()?;
    let addrs: Vec<SocketAddr> = eps
        .iter()
        .map(UdpEndpoint::local_addr)
        .collect::<io::Result<_>>()?;
    let replica_ids: Vec<PeerId> = (0..cfg.replicas as PeerId).collect();

    let total_ops = cfg.ops_per_client * cfg.clients as u64;
    let done = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();

    for (i, ep) in eps.into_iter().enumerate() {
        let me = i as PeerId;
        let peers: Vec<(PeerId, SocketAddr)> = (0..n)
            .filter(|&j| j != i)
            .map(|j| (j as PeerId, addrs[j]))
            .collect();
        let is_replica = (i as usize) < cfg.replicas;
        let replica_ids = replica_ids.clone();
        let cfg = *cfg;
        let done = Arc::clone(&done);
        let dir = dir.to_path_buf();

        handles.push(thread::spawn(move || -> io::Result<Option<(PeerId, Vec<(String, i64)>)>> {
            let mut host = ReliableHost::new(ep, me, &peers, cfg.host);
            let started = Instant::now();

            if is_replica {
                let mut eng = Engine::<KvStore>::create(dir.join(format!("replica-{me}")), cfg.snap_interval, false)?;
                let mut applied = 0u64;
                loop {
                    for d in host.poll(Duration::from_millis(1)).unwrap_or_default() {
                        if let Some(op) = Op::decode(&d.payload) {
                            if !matches!(eng.deliver(d.msg_ts, &op)?, crate::Output::Suppressed) {
                                applied += 1;
                            }
                        }
                    }
                    if applied >= total_ops {
                        done.store(true, Ordering::SeqCst);
                    }
                    // Stay alive until everyone is done (keep beaconing so peers'
                    // barriers can advance), then exit.
                    if (done.load(Ordering::SeqCst) && applied >= total_ops)
                        || started.elapsed() >= cfg.timeout
                    {
                        // Drain a little to let stragglers land.
                        let drain_end = Instant::now() + Duration::from_millis(50);
                        while Instant::now() < drain_end {
                            for d in host.poll(Duration::from_millis(1)).unwrap_or_default() {
                                if let Some(op) = Op::decode(&d.payload) {
                                    let _ = eng.deliver(d.msg_ts, &op)?;
                                }
                            }
                        }
                        break;
                    }
                }
                Ok(Some((me, eng.state().entries())))
            } else {
                // Client: scatter its ops to all replicas, then keep the fabric
                // live (beacon) until the cluster is done.
                let ops = client_ops(me as u32, cfg.ops_per_client, cfg.keys);
                for op in &ops {
                    host.send(&replica_ids, &op.encode());
                    // Pump receives so acks/barriers flow; small pacing.
                    let _ = host.poll(Duration::from_micros(200));
                }
                loop {
                    let _ = host.poll(Duration::from_millis(1));
                    if done.load(Ordering::SeqCst) || started.elapsed() >= cfg.timeout {
                        break;
                    }
                }
                Ok(None)
            }
        }));
    }

    let mut replica_states: Vec<(PeerId, Vec<(String, i64)>)> = Vec::new();
    for h in handles {
        if let Some(s) = h.join().unwrap()? {
            replica_states.push(s);
        }
    }
    replica_states.sort_by_key(|(id, _)| *id);

    let expected = expected_state(cfg.clients, cfg.ops_per_client, cfg.keys);
    let converged = replica_states
        .windows(2)
        .all(|w| w[0].1 == w[1].1)
        && !replica_states.is_empty();
    let correct = replica_states.iter().all(|(_, s)| s == &expected);
    let timed_out = !correct;

    Ok(ClusterReport {
        replica_states,
        expected,
        converged,
        correct,
        timed_out,
    })
}

/// Outcome of a crash run.
#[derive(Clone, Debug)]
pub struct CrashReport {
    pub expected: Vec<(String, i64)>,
    /// A survivor's final state (the fabric recovered from the replica crash).
    pub survivor_state: Vec<(String, i64)>,
    /// The victim recovered from its own durable store (a consistent prefix).
    pub victim_prefix: Vec<(String, i64)>,
    /// The victim after a state transfer from the survivor (caught up).
    pub victim_caught_up: Vec<(String, i64)>,
    pub survivor_correct: bool,
    /// Victim's recovered prefix is a consistent under-approximation (each key
    /// ≤ expected) — no corruption, no over-count.
    pub victim_prefix_consistent: bool,
    pub recovered_ok: bool,
}

/// RQ3/RQ4 on the live fabric: a replica crashes mid-run; survivors keep the
/// total order and reach the correct state (the fabric excises the dead peer);
/// the victim recovers its durable prefix and catches up via state transfer.
pub fn run_cluster_with_crash(
    cfg: &ClusterConfig,
    dir: &std::path::Path,
    victim: PeerId,
    crash_after: u64,
) -> io::Result<CrashReport> {
    use std::sync::atomic::AtomicUsize;
    let n = cfg.replicas + cfg.clients;
    let eps: Vec<UdpEndpoint> = (0..n).map(|_| UdpEndpoint::bind(loopback())).collect::<io::Result<_>>()?;
    let addrs: Vec<SocketAddr> = eps.iter().map(UdpEndpoint::local_addr).collect::<io::Result<_>>()?;
    let replica_ids: Vec<PeerId> = (0..cfg.replicas as PeerId).collect();
    let total = cfg.ops_per_client * cfg.clients as u64;
    let n_survivors = cfg.replicas - 1;
    let survivors_done = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for (i, ep) in eps.into_iter().enumerate() {
        let me = i as PeerId;
        let peers: Vec<(PeerId, SocketAddr)> =
            (0..n).filter(|&j| j != i).map(|j| (j as PeerId, addrs[j])).collect();
        let is_replica = (i as usize) < cfg.replicas;
        let is_victim = is_replica && me == victim;
        let replica_ids = replica_ids.clone();
        let cfg = *cfg;
        let survivors_done = Arc::clone(&survivors_done);
        let dir = dir.to_path_buf();

        handles.push(thread::spawn(move || -> io::Result<Option<(PeerId, Vec<(String, i64)>, Vec<u8>, bool)>> {
            let mut host = ReliableHost::new(ep, me, &peers, cfg.host);
            let started = Instant::now();
            if is_replica {
                let mut eng = Engine::<KvStore>::create(dir.join(format!("replica-{me}")), cfg.snap_interval, false)?;
                let mut applied = 0u64;
                loop {
                    for d in host.poll(Duration::from_millis(1)).unwrap_or_default() {
                        if let Some(op) = Op::decode(&d.payload) {
                            if !matches!(eng.deliver(d.msg_ts, &op)?, crate::Output::Suppressed) {
                                applied += 1;
                            }
                        }
                    }
                    if is_victim && applied >= crash_after {
                        // Crash: stop serving (drop socket) with a durable prefix.
                        return Ok(Some((me, eng.state().entries(), eng.export_state(), true)));
                    }
                    if !is_victim && applied >= total {
                        survivors_done.fetch_add(1, Ordering::SeqCst);
                        let drain = Instant::now() + Duration::from_millis(50);
                        while Instant::now() < drain {
                            for d in host.poll(Duration::from_millis(1)).unwrap_or_default() {
                                if let Some(op) = Op::decode(&d.payload) {
                                    let _ = eng.deliver(d.msg_ts, &op)?;
                                }
                            }
                        }
                        return Ok(Some((me, eng.state().entries(), eng.export_state(), false)));
                    }
                    if started.elapsed() >= cfg.timeout {
                        return Ok(Some((me, eng.state().entries(), eng.export_state(), false)));
                    }
                }
            } else {
                let c = me as u32;
                for i in 0..cfg.ops_per_client {
                    let op = Op::incr(c, i + 1, &format!("k{}", i % cfg.keys), 1);
                    host.send(&replica_ids, &op.encode());
                    let t = Instant::now() + Duration::from_micros(300);
                    while Instant::now() < t {
                        let _ = host.poll(Duration::from_micros(50));
                    }
                }
                while survivors_done.load(Ordering::SeqCst) < n_survivors && started.elapsed() < cfg.timeout {
                    let _ = host.poll(Duration::from_millis(1));
                }
                Ok(None)
            }
        }));
    }

    let mut survivor: Option<(Vec<(String, i64)>, Vec<u8>)> = None;
    let mut victim_prefix: Vec<(String, i64)> = Vec::new();
    for h in handles {
        if let Some((_id, entries, export, crashed)) = h.join().unwrap()? {
            if crashed {
                victim_prefix = entries;
            } else if survivor.is_none() {
                survivor = Some((entries, export));
            }
        }
    }
    let (survivor_state, survivor_export) = survivor.expect("at least one survivor");
    let expected = expected_state(cfg.clients, cfg.ops_per_client, cfg.keys);

    // Victim recovers its durable prefix, then state-transfers from the survivor.
    let mut veng = Engine::<KvStore>::recover(dir.join(format!("replica-{victim}")), cfg.snap_interval, false)?;
    veng.import_state(&survivor_export)?;
    let victim_caught_up = veng.state().entries();

    let exp_map: std::collections::BTreeMap<_, _> = expected.iter().cloned().collect();
    let victim_prefix_consistent = victim_prefix.iter().all(|(k, v)| exp_map.get(k).is_some_and(|e| v <= e));
    let survivor_correct = survivor_state == expected;
    let recovered_ok = victim_caught_up == survivor_state && survivor_correct;

    Ok(CrashReport {
        expected,
        survivor_state,
        victim_prefix,
        victim_caught_up,
        survivor_correct,
        victim_prefix_consistent,
        recovered_ok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("ob-cluster-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn survivor_stays_correct_and_victim_recovers_after_a_replica_crash() {
        // RQ3/RQ4 on the live fabric. 3 replicas; replica 1 crashes after 60 ops.
        // The fabric excises it; survivors reach the exact expected state; the
        // victim recovers a consistent prefix and catches up via state transfer.
        let dir = tmpdir("crash");
        let cfg = ClusterConfig {
            replicas: 3,
            clients: 2,
            ops_per_client: 150,
            keys: 8,
            snap_interval: 32,
            host: HostConfig {
                // Excise a dead destination quickly so survivors' commit barrier
                // resumes (still well above loopback scheduling jitter).
                failure_timeout: Duration::from_millis(600),
                ..ClusterConfig::default().host
            },
            timeout: Duration::from_secs(40),
            ..Default::default()
        };
        let r = run_cluster_with_crash(&cfg, &dir, 1, 60).unwrap();
        assert!(r.survivor_correct, "survivor wrong after crash: {:?} vs {:?}", r.survivor_state, r.expected);
        assert!(r.victim_prefix_consistent, "victim prefix inconsistent: {:?}", r.victim_prefix);
        assert!(r.recovered_ok, "victim did not recover+catch up: {:#?}", r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replicas_converge_to_exact_state_over_live_fabric() {
        let dir = tmpdir("converge");
        let cfg = ClusterConfig {
            replicas: 3,
            clients: 2,
            ops_per_client: 150,
            keys: 8,
            snap_interval: 64,
            ..Default::default()
        };
        let r = run_cluster(&cfg, &dir).unwrap();
        assert!(r.converged, "replicas diverged: {:#?}", r.replica_states);
        assert!(r.correct, "state != expected\n  got: {:#?}\n  exp: {:?}", r.replica_states, r.expected);
        // 2 clients x 150 ops = 300 increments spread over 8 keys.
        let total: i64 = r.expected.iter().map(|(_, v)| v).sum();
        assert_eq!(total, 300);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
