//! `ob-cpu` — RQ5: execution CPU, OneBarrier passive vs active SMR. Active SMR
//! runs the state machine on every replica (N× execution CPU); OneBarrier passive
//! runs it once and keeps the other replicas as log-only backups, so execution
//! CPU stays ≈ 1× regardless of replication factor (docs/PLAN.md §7 RQ5).

use onebarrier::bench::bench_cpu_passive_vs_active;

fn main() {
    let dir = std::env::temp_dir().join(format!("ob-cpu-{}", std::process::id()));
    let ops = 5_000u64;
    let apply_us = 20u64; // model a non-trivial state-machine apply

    println!("OneBarrier RQ5 — execution CPU: passive vs active SMR");
    println!("  {ops} ops, ~{apply_us}µs apply cost each\n");
    println!("{:>9} {:>16} {:>18} {:>10}", "replicas", "active-SMR CPU ms", "passive(OB) CPU ms", "savings");
    println!("{}", "-".repeat(58));
    for replicas in [2usize, 3, 5, 7] {
        let d = dir.join(format!("r{replicas}"));
        let (active, passive) = bench_cpu_passive_vs_active(&d, ops, apply_us, replicas);
        let savings = 100.0 * (1.0 - passive.exec_cpu_ms / active.exec_cpu_ms);
        println!("{:>9} {:>16.1} {:>18.1} {:>9.0}%", replicas, active.exec_cpu_ms, passive.exec_cpu_ms, savings);
    }
    println!("\nRead-out: active SMR's execution CPU grows ~linearly with the replica");
    println!("count; OneBarrier passive keeps it ≈ 1× (backups only log, never execute).");
    let _ = std::fs::remove_dir_all(&dir);
}
