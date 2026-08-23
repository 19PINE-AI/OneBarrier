//! `ob-sweep` — RQ8: the snapshot-interval tradeoff. Smaller interval ⇒ more
//! snapshots (higher steady-state overhead) but fewer replay records on recovery
//! (faster recovery); larger interval ⇒ the reverse. This is the empirical shape
//! behind the recovery model's `I*` rule (docs/research/PLAN.md §7).

use onebarrier::bench::sweep_snapshot_interval;

fn main() {
    let dir = std::env::temp_dir().join(format!("ob-sweep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ops = 50_000u64;
    // Deliberately not divisors of `ops` (so a partial window remains = replay
    // records on recovery), and a final interval > ops (no snapshot ⇒ full replay).
    let intervals = [64u64, 512, 4_096, 100_000];

    println!("OneBarrier RQ8 — snapshot-interval tradeoff ({ops} ops, 64 keys)\n");
    println!("{:>10} {:>11} {:>16} {:>14} {:>13}", "interval", "snapshots", "apply µs/op", "replay recs", "recover µs");
    println!("{}", "-".repeat(68));
    let rows = sweep_snapshot_interval(&dir, &intervals, ops, 64);
    for r in &rows {
        println!("{:>10} {:>11} {:>16.3} {:>14} {:>13.1}", r.interval, r.snapshots, r.apply_us_per_op, r.replay_records, r.recover_us);
    }
    println!("\nRead-out: snapshot count falls and replay-on-recovery rises with the");
    println!("interval — the steady-state-overhead vs recovery-cost tradeoff (RQ8).");
    let _ = std::fs::remove_dir_all(&dir);
}
