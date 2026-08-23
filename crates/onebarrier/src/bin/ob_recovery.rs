//! `ob-recovery` — recovery time vs load, the livelock regime, and the barrier-hold
//! backpressure fix (docs/research/PAPER-PLAN.md exp #6). SIMULATED via the
//! recovery model;
//! 1Pipe detection + the catch-up dynamics.
use onebarrier::recovery::{recovery_time_us, sweep, RecoveryParams};
fn main() {
    let p = RecoveryParams::default();

    // `ob-recovery csv` emits the fine sweep used by fig_recovery_load: recovery
    // time vs live load, plain vs barrier-hold backpressure, with the livelock
    // wall at live_rate >= replay capacity. Livelock is emitted as an empty field.
    if std::env::args().any(|a| a == "csv") {
        println!("live_rate_ops_us,plain_us,backpressure_us,replay_rate");
        let n = 60;
        for i in 0..n {
            let lr = 0.2 + (i as f64 / (n - 1) as f64) * (8.0 - 0.2);
            let plain = recovery_time_us(&p, lr, false).map(|v| format!("{v:.1}")).unwrap_or_default();
            let bp = recovery_time_us(&p, lr, true).map(|v| format!("{v:.1}")).unwrap_or_default();
            println!("{lr:.3},{plain},{bp},{}", p.replay_rate);
        }
        return;
    }

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
