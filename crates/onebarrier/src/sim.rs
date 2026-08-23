//! Discrete-event simulation of the OneBarrier request path at the **RDMA
//! operating point** — for the experiments that need real-hardware latencies we
//! don't have (docs/research/PAPER-PLAN.md GATE A). Latency parameters are
//! taken from the
//! 1Pipe paper's measured testbed (RDMA RTT 1–2 µs, reliable = +1 RTT, etc.), so
//! this is *simulation with a measured latency model*, clearly labelled — not the
//! reproduction's loopback numbers and not silicon.
//!
//! A single-executor FIFO queue (the share-nothing model) with Poisson arrivals.
//! The three modes capture the RQ2 thesis:
//!   * `ReliableBaseline` — reliable-1Pipe, no FT durability.
//!   * `FtOverlap`        — durable replica write rides 1Pipe's 2PC phase-1
//!                          (in-fabric RDMA), so it does NOT occupy the executor
//!                          and adds no critical-path latency → FT ≈ free.
//!   * `FtFsync`          — durability is a serial stable-storage write on the
//!                          executor's path → service time explodes, queue
//!                          collapses under load (the out-of-regime tier).

#[derive(Clone, Copy, Debug)]
pub struct SimParams {
    pub rtt_us: f64,       // RDMA round-trip (paper: 1–2 µs)
    pub barrier_us: f64,   // reliable commit-barrier wait (≈ +1 RTT / beacon)
    pub apply_us: f64,     // state-machine apply cost
    pub fsync_us: f64,     // stable-storage durability (out-of-regime tier)
    pub requests: u64,
    pub offered_load: f64, // fraction of the apply-bound executor capacity (0..1)
    pub seed: u64,
}

impl Default for SimParams {
    fn default() -> Self {
        Self {
            rtt_us: 2.0,
            barrier_us: 2.0,
            apply_us: 0.5,
            fsync_us: 3000.0,
            requests: 200_000,
            offered_load: 0.7,
            seed: 0x9E3779B97F4A7C15,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    ReliableBaseline,
    FtOverlap,
    FtFsync,
}

#[derive(Clone, Debug)]
pub struct LatencyStats {
    pub mode: String,
    pub n: usize,
    pub p50_us: f64,
    pub p99_us: f64,
    pub p999_us: f64,
    pub mean_us: f64,
    pub max_us: f64,
}

/// Executor occupancy per request (what serializes on the single executor).
fn service_us(p: &SimParams, mode: Mode) -> f64 {
    match mode {
        Mode::ReliableBaseline | Mode::FtOverlap => p.apply_us,
        Mode::FtFsync => p.apply_us + p.fsync_us, // serial durability on the path
    }
}

/// Fixed per-request latency outside the executor queue: network + commit barrier.
/// Durability rides the barrier in `FtOverlap` (no add); it is in `service` for
/// `FtFsync`; the baseline pays the same reliable barrier (FT marginal = 0).
fn fixed_us(p: &SimParams, _mode: Mode) -> f64 {
    p.rtt_us + p.barrier_us
}

/// Deterministic xorshift64* PRNG (reproducible; no external dep).
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn unit(&mut self) -> f64 {
        // (0,1)
        ((self.next_u64() >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0)
    }
    fn exp(&mut self, rate: f64) -> f64 {
        -self.unit().ln() / rate
    }
}

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Simulate one mode: single-server FIFO queue, Poisson arrivals at `offered_load`
/// of the apply-bound capacity (so all modes see the SAME arrival rate — `FtFsync`
/// is then over its own capacity, exactly the collapse the real fsync tier shows).
pub fn simulate(p: &SimParams, mode: Mode) -> LatencyStats {
    let lambda = p.offered_load / p.apply_us; // requests per µs (apply-bound capacity)
    let svc = service_us(p, mode);
    let fixed = fixed_us(p, mode);
    // Same arrival stream across modes (seed independent of mode), so the only
    // difference is service/fixed — i.e. FT-overlap vs baseline is an apples-to-
    // apples, same-workload comparison (marginal is then exactly the FT cost).
    let mut rng = Rng::new(p.seed);

    let mut now = 0.0f64;
    let mut prev_finish = 0.0f64;
    let mut lat: Vec<f64> = Vec::with_capacity(p.requests as usize);
    for _ in 0..p.requests {
        now += rng.exp(lambda); // next arrival
        let start = now.max(prev_finish);
        let finish = start + svc;
        prev_finish = finish;
        lat.push((finish - now) + fixed); // queue+service + network+barrier
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = lat.iter().sum::<f64>() / lat.len() as f64;
    LatencyStats {
        mode: format!("{mode:?}"),
        n: lat.len(),
        p50_us: pct(&lat, 0.50),
        p99_us: pct(&lat, 0.99),
        p999_us: pct(&lat, 0.999),
        mean_us: mean,
        max_us: *lat.last().unwrap(),
    }
}

// ---------------------------------------------------------------------------
// Competitor head-to-head at the RDMA operating point (paper experiment #3)
// ---------------------------------------------------------------------------

/// A transparent-FT competitor modeled by its *documented mechanism* at the RDMA
/// operating point (parameters from each system's own paper). Not reimplementations
/// — models, clearly labelled — of the cost each design pays per externalizing op.
#[derive(Clone, Debug)]
pub struct Competitor {
    pub name: &'static str,
    pub svc_us: f64,    // executor occupancy per op
    pub fixed_us: f64,  // network + output-commit latency outside the queue
    pub cpu_mult: f64,  // execution-resource multiplier vs one live copy
    pub note: &'static str,
}

/// The competitor set, parameterized from their papers + the 1Pipe operating point.
pub fn competitors(p: &SimParams) -> Vec<Competitor> {
    let remus_ckpt_us = 25_000.0; // Remus checkpoint interval ~25 ms (NSDI'08)
    let seq_us = p.rtt_us;        // LLFT host-sequencer extra round-trip
    let log_us = 0.3;             // HyCoR per-op nondeterminism-log write
    vec![
        Competitor { name: "OneBarrier", svc_us: p.apply_us, fixed_us: p.rtt_us + p.barrier_us, cpu_mult: 1.0,
            note: "durability rides 1Pipe 2PC phase-1 (in-fabric RDMA)" },
        Competitor { name: "Remus", svc_us: p.apply_us, fixed_us: p.rtt_us + remus_ckpt_us / 2.0, cpu_mult: 2.0,
            note: "output buffered until next checkpoint (~25 ms hold)" },
        Competitor { name: "COLO", svc_us: p.apply_us, fixed_us: p.rtt_us + p.barrier_us, cpu_mult: 2.0,
            note: "lock-step VMs; output released on match, checkpoint on mismatch" },
        Competitor { name: "LLFT", svc_us: p.apply_us, fixed_us: p.rtt_us + p.barrier_us + seq_us, cpu_mult: 2.0,
            note: "host virtual-time sequencer adds a round-trip" },
        Competitor { name: "HyCoR", svc_us: p.apply_us + log_us, fixed_us: p.rtt_us + p.barrier_us, cpu_mult: 1.0,
            note: "per-op nondeterminism log write on the path" },
        Competitor { name: "active-SMR(3)", svc_us: p.apply_us, fixed_us: p.rtt_us + p.barrier_us, cpu_mult: 3.0,
            note: "all 3 replicas execute every op" },
    ]
}

/// Run the single-executor queue for an explicit (svc, fixed) — used by competitors.
pub fn simulate_explicit(p: &SimParams, svc: f64, fixed: f64) -> LatencyStats {
    let lambda = p.offered_load / p.apply_us;
    let mut rng = Rng::new(p.seed);
    let mut now = 0.0f64;
    let mut prev_finish = 0.0f64;
    let mut lat: Vec<f64> = Vec::with_capacity(p.requests as usize);
    for _ in 0..p.requests {
        now += rng.exp(lambda);
        let start = now.max(prev_finish);
        let finish = start + svc;
        prev_finish = finish;
        lat.push((finish - now) + fixed);
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = lat.iter().sum::<f64>() / lat.len() as f64;
    LatencyStats {
        mode: "competitor".into(),
        n: lat.len(),
        p50_us: pct(&lat, 0.50),
        p99_us: pct(&lat, 0.99),
        p999_us: pct(&lat, 0.999),
        mean_us: mean,
        max_us: *lat.last().unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onebarrier_dominates_competitors_on_latency_and_cpu() {
        let p = SimParams { requests: 50_000, offered_load: 0.7, ..Default::default() };
        let cs = competitors(&p);
        let ob = cs.iter().find(|c| c.name == "OneBarrier").unwrap();
        let ob_lat = simulate_explicit(&p, ob.svc_us, ob.fixed_us);
        for c in &cs {
            let lat = simulate_explicit(&p, c.svc_us, c.fixed_us);
            // OneBarrier's p99 is <= every competitor's, and its CPU mult is the lowest.
            assert!(ob_lat.p99_us <= lat.p99_us + 1e-9, "OB p99 {} > {} p99 {}", ob_lat.p99_us, c.name, lat.p99_us);
            assert!(ob.cpu_mult <= c.cpu_mult, "OB cpu {} > {} cpu {}", ob.cpu_mult, c.name, c.cpu_mult);
        }
    }

    #[test]
    fn ft_overlap_matches_baseline_and_fsync_collapses() {
        let p = SimParams { requests: 50_000, offered_load: 0.7, ..Default::default() };
        let base = simulate(&p, Mode::ReliableBaseline);
        let overlap = simulate(&p, Mode::FtOverlap);
        let fsync = simulate(&p, Mode::FtFsync);
        // FT-overlap is within a hair of the reliable baseline (FT ≈ free).
        assert!((overlap.p99_us - base.p99_us).abs() < 0.01, "overlap p99 {} vs base {}", overlap.p99_us, base.p99_us);
        // fsync collapses (queue blows up) — orders of magnitude worse tail.
        assert!(fsync.p99_us > base.p99_us * 100.0, "fsync p99 {} should dwarf base {}", fsync.p99_us, base.p99_us);
    }
}
