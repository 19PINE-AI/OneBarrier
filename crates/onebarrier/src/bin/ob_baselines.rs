//! `ob-baselines` — M4: transparent-FT order-establishment head-to-head.
//! OneBarrier (fabric order, no order-log) vs LLFT (host virtual-time sequencer)
//! vs HyCoR (per-op non-determinism logging). Same apply+append work; the delta
//! is the ordering mechanism (docs/research/PLAN.md §3, §7 RQ7).

use onebarrier::bench::bench_ft_baselines;

fn main() {
    let dir = std::env::temp_dir().join(format!("ob-baselines-{}", std::process::id()));
    let ops = 50_000u64;
    println!("OneBarrier M4 — transparent-FT order establishment, head-to-head");
    println!("  {ops} ops/producer (software model on the reproduction)\n");
    println!("{:>9} {:>30} {:>30} {:>30}", "producers", "OneBarrier ops/s", "HyCoR ops/s", "LLFT ops/s");
    println!("{}", "-".repeat(102));
    for threads in [1usize, 2, 4, 8] {
        let d = dir.join(format!("t{threads}"));
        let r = bench_ft_baselines(&d, threads, ops);
        let get = |p: &str| r.iter().find(|x| x.mode.starts_with(p)).unwrap().ops_per_sec;
        println!("{:>9} {:>30.0} {:>30.0} {:>30.0}", threads, get("OneBarrier"), get("HyCoR"), get("LLFT"));
    }
    println!("\nRead-out: OneBarrier pays no per-op ordering cost (the fabric supplies the");
    println!("order); HyCoR pays an extra per-op order-log write; LLFT serializes every");
    println!("op through its host sequencer and degrades under producer contention.");
    let _ = std::fs::remove_dir_all(&dir);
}
