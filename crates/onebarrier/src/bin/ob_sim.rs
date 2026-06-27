//! `ob-sim` — RQ2 + tail latency at the RDMA operating point (simulation with the
//! 1Pipe paper's measured latency model; docs/PAPER-PLAN.md GATE A). Single
//! executor, Poisson arrivals; sweeps offered load to show FT-overlap tracks the
//! reliable-1Pipe baseline (FT ≈ free) while serial-fsync durability collapses.

use onebarrier::sim::{simulate, Mode, SimParams};

fn main() {
    let base = SimParams { rtt_us: 2.0, barrier_us: 2.0, apply_us: 0.5, fsync_us: 3000.0, requests: 300_000, ..Default::default() };

    // `ob-sim csv` emits the fine-grained latency-vs-throughput sweep used by
    // Figure fig_loadsweep: each tier is swept across offered load and goes
    // vertical at its OWN saturation throughput (overlap/baseline at the
    // apply-bound knee, fsync at ~1/6000th of it). Plotted log-log so the
    // unstable region is a wall, not a degenerate number.
    if std::env::args().any(|a| a == "csv") {
        println!("offered_load,throughput_ops_s,baseline_p999_us,overlap_p999_us,fsync_p999_us");
        let n = 48;
        let (lo, hi) = (5e-5_f64, 0.97_f64);
        for i in 0..n {
            let load = lo * (hi / lo).powf(i as f64 / (n - 1) as f64);
            let p = SimParams { offered_load: load, requests: 120_000, ..base };
            let thr = load / p.apply_us * 1.0e6; // arrival rate -> ops/s
            let b = simulate(&p, Mode::ReliableBaseline).p999_us;
            let o = simulate(&p, Mode::FtOverlap).p999_us;
            let f = simulate(&p, Mode::FtFsync).p999_us;
            println!("{load:.6},{thr:.1},{b:.4},{o:.4},{f:.4}");
        }
        return;
    }

    println!("OneBarrier — RQ2 + tail latency at the RDMA operating point (SIMULATED)");
    println!("  latency model from the 1Pipe paper: RDMA RTT {} µs, reliable barrier {} µs,", base.rtt_us, base.barrier_us);
    println!("  apply {} µs; single executor, Poisson arrivals. Absolute µs are SIMULATED,", base.apply_us);
    println!("  not silicon — the SHAPE (overlap ≈ free, fsync collapses) is the result.\n");

    println!("{:>6} {:>26} {:>26} {:>26}", "load", "reliable-1Pipe p50/p99/p99.9", "FT-overlap p50/p99/p99.9", "FT-fsync p50/p99/p99.9 (µs)");
    println!("{}", "-".repeat(88));
    for load in [0.3, 0.5, 0.7, 0.85, 0.95] {
        let p = SimParams { offered_load: load, ..base };
        let b = simulate(&p, Mode::ReliableBaseline);
        let o = simulate(&p, Mode::FtOverlap);
        let f = simulate(&p, Mode::FtFsync);
        let fmt = |s: &onebarrier::sim::LatencyStats| format!("{:.1}/{:.1}/{:.1}", s.p50_us, s.p99_us, s.p999_us);
        println!("{:>6.2} {:>26} {:>26} {:>26}", load, fmt(&b), fmt(&o), fmt(&f));
    }

    let p = SimParams { offered_load: 0.7, ..base };
    let b = simulate(&p, Mode::ReliableBaseline);
    let o = simulate(&p, Mode::FtOverlap);
    let f = simulate(&p, Mode::FtFsync);
    println!("\nRQ2 @ RDMA (load 0.7):");
    println!("  reliable-1Pipe baseline   p99 = {:.2} µs", b.p99_us);
    println!("  OneBarrier FT (overlap)   p99 = {:.2} µs   (marginal over baseline = {:.3} µs ≈ free)", o.p99_us, o.p99_us - b.p99_us);
    println!("  OneBarrier FT (fsync)     p99 = {:.0} µs   ({:.0}x the baseline — out of regime)", f.p99_us, f.p99_us / b.p99_us);
    println!("\nTail money-graph read-out: FT-overlap's p99.9 tracks the reliable-1Pipe");
    println!("baseline across all loads (the output-commit barrier IS the durability");
    println!("barrier at µs scale); serial-fsync durability collapses the tail — the");
    println!("very failure mode (output-hold tail) that sank Remus.");
}
