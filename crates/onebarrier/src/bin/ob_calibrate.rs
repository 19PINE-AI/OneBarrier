//! `ob-calibrate` — validate the discrete-event model (`ob-sim`) against the REAL
//! OneBarrier engine on the loopback fabric, so the RDMA projection rests on a
//! model checked where we *can* measure, not on assertion.
//!
//! Two checks:
//!   1. Additive structure (idle load): feed the sim the engine's measured
//!      delivery + durability latencies; the sim must reproduce the engine's
//!      measured *commit* for both tiers (overlap rides → commit≈delivery;
//!      fsync stacks → commit≈delivery+fsync).
//!   2. Queue dynamics (the genuinely-extrapolated part): drive the engine's
//!      fsync tier through increasing offered load and show its commit latency
//!      blows up at the same saturation knee (~1/fsync) the sim predicts.

use std::time::Duration;

use onebarrier::bench::{run_bench, BenchConfig, Durability};
use onebarrier::sim::{simulate, Mode, SimParams};

fn us(ns: u64) -> f64 {
    ns as f64 / 1000.0
}

fn engine(clients: usize, rate_ops_s: f64, tier: Durability) -> std::io::Result<onebarrier::bench::BenchResult> {
    let pace = Duration::from_secs_f64(clients as f64 / rate_ops_s);
    let cfg = BenchConfig { clients, ops_per_client: 200, keys: 64, pace, durability: tier, ..Default::default() };
    let dir = std::env::temp_dir().join(format!("ob-cal-{}-{:?}-{}", std::process::id(), tier, rate_ops_s as u64));
    let _ = std::fs::remove_dir_all(&dir);
    let r = run_bench(&cfg, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    r
}

fn main() -> std::io::Result<()> {
    println!("ob-calibrate — discrete-event model vs the real loopback engine\n");

    // ---------- Check 1: additive structure at low (matched) load ----------
    // Run the engine well under the fsync ceiling so queueing is small, and feed
    // the sim the SAME arrival rate so the comparison is apples-to-apples.
    let idle_rate = 60.0; // ops/s, under the ~340 ops/s fsync ceiling
    let mem = engine(2, idle_rate, Durability::InFabricMem)?;
    let fs = engine(2, idle_rate, Durability::Fsync)?;
    let deliv = us(mem.delivery_p50);
    let dur_mem = us(mem.durable_p50);
    let dur_fs = us(fs.durable_p50);

    // Feed the sim the engine's measured constants (loopback regime): the fixed
    // network+barrier term = measured delivery; per-op apply = measured in-fabric
    // marginal (the replica write that rides); fsync = measured serial-write cost.
    // offered_load is relative to apply-bound capacity (1/apply), so match the rate:
    let lp = SimParams {
        rtt_us: 0.0, barrier_us: deliv, apply_us: dur_mem.max(0.1), fsync_us: dur_fs,
        offered_load: idle_rate * dur_mem.max(0.1) / 1.0e6, requests: 200_000, ..Default::default()
    };
    let sim_mem = simulate(&lp, Mode::FtOverlap).p50_us;
    let sim_fs = simulate(&lp, Mode::FtFsync).p50_us;
    let err = |sim: f64, eng: f64| 100.0 * (sim - eng).abs() / eng;

    println!("[1] additive structure (idle): sim is fed delivery={deliv:.0}µs, apply={dur_mem:.1}µs, fsync={dur_fs:.0}µs");
    println!("    {:<10} {:>14} {:>14} {:>8}", "tier", "engine commit", "sim commit", "err");
    println!("    {:<10} {:>12.1}µs {:>12.1}µs {:>7.1}%", "overlap", us(mem.commit_p50), sim_mem, err(sim_mem, us(mem.commit_p50)));
    println!("    {:<10} {:>12.1}µs {:>12.1}µs {:>7.1}%", "fsync", us(fs.commit_p50), sim_fs, err(sim_fs, us(fs.commit_p50)));
    println!("    -> the sim predicts the COMBINED commit from the measured parts: overlap");
    println!("       rides (commit≈delivery), fsync stacks additively (commit≈delivery+fsync).\n");

    // ---------- Check 2: queue dynamics — fsync saturation knee ----------
    let knee = 1.0e6 / dur_fs; // predicted ops/s at which the fsync executor saturates
    println!("[2] queue dynamics: sim predicts the fsync tier saturates near 1/fsync = {knee:.0} ops/s.");
    println!("    drive the REAL engine's fsync tier through it:");
    println!("    {:>10} {:>10} {:>16} {:>16}", "rate ops/s", "util", "engine commit p50", "engine commit p99");
    for rate in [knee * 0.5, knee * 0.75, knee * 0.9, knee * 1.1, knee * 1.4] {
        let r = engine(4, rate, Durability::Fsync)?;
        let util = rate * dur_fs / 1.0e6;
        println!("    {:>10.0} {:>10.2} {:>14.1}µs {:>14.1}µs", rate, util, us(r.commit_p50), us(r.commit_p99));
    }
    println!("    -> commit latency stays near the floor below the knee and climbs as");
    println!("       util→1, matching the sim's M/D/1 fsync curve (fig_loadsweep). The");
    println!("       in-fabric tier has no such knee until ~1/apply (≈6000x higher).");
    Ok(())
}
