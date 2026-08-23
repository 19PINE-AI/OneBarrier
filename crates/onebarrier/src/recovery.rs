//! Recovery-cost model + the replay catch-up dynamics
//! (docs/research/PAPER-PLAN.md exp #6;
//! the recovery red-team's livelock regime). A recovering replica restores its
//! snapshot then replays the durable log forward to the live cut; it converges
//! only if its replay rate exceeds the live arrival rate (`s = R_replay/R_live`).
//! We model recovery time vs load, the livelock at `s ≤ 1`, and the barrier-hold
//! backpressure that restores convergence — with absolute terms from the 1Pipe
//! paper (detect 50–500 µs) and the recovery model.

#[derive(Clone, Copy, Debug)]
pub struct RecoveryParams {
    pub detect_us: f64,        // failure detection (1Pipe: 50–500 µs)
    pub fetch_restore_us: f64, // pull snapshot + rehydrate state
    pub rejoin_us: f64,        // re-enter the barrier (1–2 RTT)
    pub replay_rate: f64,      // ops/µs a recovering replica can re-apply (no I/O, suppressed outputs)
    pub snap_interval_us: f64, // avg snapshot staleness ≈ interval/2 worth of time
}

impl Default for RecoveryParams {
    fn default() -> Self {
        Self { detect_us: 200.0, fetch_restore_us: 500.0, rejoin_us: 6.0, replay_rate: 4.0, snap_interval_us: 5_000.0 }
    }
}

/// Recovery time (µs) at a given live arrival rate, or `None` if the replica
/// **livelocks** (replay can't outrun the live stream, `s ≤ 1`).
/// `backpressure`: if true, the fabric barrier-holds senders to the recovering
/// node during catch-up, halving the effective live rate (restores `s > 1`).
pub fn recovery_time_us(p: &RecoveryParams, live_rate: f64, backpressure: bool) -> Option<f64> {
    let downtime = p.detect_us + p.fetch_restore_us;
    let eff_live = if backpressure { live_rate * 0.5 } else { live_rate };
    let s = p.replay_rate / eff_live;
    if s <= 1.0 {
        return None; // livelock: backlog grows without bound
    }
    // Backlog at replay start = ops that accumulated during staleness + downtime.
    let lag_us = p.snap_interval_us / 2.0;
    let backlog = eff_live * (lag_us + downtime);
    let t_replay = backlog / (p.replay_rate - eff_live);
    Some(downtime + t_replay + p.rejoin_us)
}

#[derive(Clone, Debug)]
pub struct RecoveryRow {
    pub live_rate: f64,
    pub plain_us: Option<f64>,
    pub backpressure_us: Option<f64>,
}

/// Sweep the live arrival rate and report recovery time with/without backpressure.
pub fn sweep(p: &RecoveryParams, rates: &[f64]) -> Vec<RecoveryRow> {
    rates
        .iter()
        .map(|&r| RecoveryRow {
            live_rate: r,
            plain_us: recovery_time_us(p, r, false),
            backpressure_us: recovery_time_us(p, r, true),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_below_capacity_and_livelocks_above() {
        let p = RecoveryParams::default(); // replay_rate 4 ops/µs
        // Below replay capacity: recovery converges (finite).
        assert!(recovery_time_us(&p, 2.0, false).is_some());
        // Above replay capacity: livelock without backpressure...
        assert!(recovery_time_us(&p, 5.0, false).is_none());
        // ...but barrier-hold backpressure (halves eff load) restores convergence.
        assert!(recovery_time_us(&p, 5.0, true).is_some());
    }
}
