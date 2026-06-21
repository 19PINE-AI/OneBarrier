//! `ob-recovery` — recovery time vs load, the livelock regime, and the barrier-hold
//! backpressure fix (docs/PAPER-PLAN.md exp #6). SIMULATED via the recovery model;
//! 1Pipe detection + the catch-up dynamics.
use onebarrier::recovery::{sweep, RecoveryParams};
fn main() {
    let p = RecoveryParams::default();
    println!("OneBarrier — recovery time vs live load (SIMULATED; replay capacity {} ops/µs)", p.replay_rate);
    println!("  detect {}µs, fetch+restore {}µs, snapshot staleness ~{}µs\n", p.detect_us, p.fetch_restore_us, p.snap_interval_us/2.0);
    println!("{:>14} {:>20} {:>22}", "live ops/µs", "recovery (plain)", "recovery (+backpressure)");
    println!("{}", "-".repeat(58));
    for row in sweep(&p, &[1.0, 2.0, 3.0, 3.8, 5.0, 7.0]) {
        let f = |o: Option<f64>| o.map_or("LIVELOCK".to_string(), |v| format!("{:.0} µs", v));
        println!("{:>14.1} {:>20} {:>22}", row.live_rate, f(row.plain_us), f(row.backpressure_us));
    }
    println!("\nRead-out: recovery is fast (sub-ms) while replay outruns the live stream");
    println!("(s = R_replay/R_live > 1); at sustained peak load it LIVELOCKS — and the");
    println!("fabric's barrier-hold backpressure (throttle senders to the recovering node)");
    println!("restores convergence. vs baselines: Redis Cluster detect ~3.3 s; Flink restore");
    println!("a minute+. OneBarrier's 1Pipe detection is 50-500 µs — recovery is detection-");
    println!("then-replay-bound, not the seconds-scale failover of native FT.");
}
