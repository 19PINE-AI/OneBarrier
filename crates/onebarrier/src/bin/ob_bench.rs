//! `ob-bench` — RQ2 (M2): output-commit latency on the live fabric, comparing the
//! in-fabric/in-memory durability tier (rides the commit barrier) against the
//! serial fsync tier (stacks on the critical path).

use std::time::Duration;

use onebarrier::bench::{run_bench, BenchConfig, Durability};

fn us(ns: u64) -> f64 {
    ns as f64 / 1000.0
}

fn main() -> std::io::Result<()> {
    let base = std::env::temp_dir().join(format!("ob-bench-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    // Load is kept well under the fsync tier's throughput ceiling (~330 ops/s,
    // since fsync ≈ 3 ms/op) so the delivery comparison is apples-to-apples and
    // neither tier queues. 2 clients × 1/8ms = 250 ops/s offered.
    let cfg = BenchConfig {
        clients: 2,
        ops_per_client: 250,
        keys: 64,
        pace: Duration::from_millis(8),
        ..Default::default()
    };

    println!("OneBarrier RQ2 — output-commit latency on the live 1Pipe fabric (loopback UDP)");
    println!("  {} clients, {} ops each, idle-paced under the fsync ceiling;", cfg.clients, cfg.ops_per_client);
    println!("  absolute µs are the reproduction, not RDMA (paper: 1-2µs RTT, 10-21µs delivery).\n");

    let mut rows = Vec::new();
    for tier in [Durability::InFabricMem, Durability::Fsync] {
        let dir = base.join(format!("{tier:?}"));
        let r = run_bench(&BenchConfig { durability: tier, ..cfg }, &dir)?;
        rows.push(r);
    }

    println!("{:<14} {:>6} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
             "tier", "n", "deliv_p50", "deliv_p99", "durable_p50", "durable_p99", "commit_p50", "commit_p99");
    println!("{}", "-".repeat(96));
    for r in &rows {
        println!("{:<14} {:>6} {:>10.2}µs {:>10.2}µs {:>10.2}µs {:>10.2}µs {:>10.2}µs {:>10.2}µs",
                 r.durability, r.n,
                 us(r.delivery_p50), us(r.delivery_p99),
                 us(r.durable_p50), us(r.durable_p99),
                 us(r.commit_p50), us(r.commit_p99));
    }

    let mem = &rows[0];
    let fs = &rows[1];
    println!("\nRQ2 read-out:");
    println!("  • fabric delivery (the reliable-1Pipe baseline)        p50 = {:.2} µs", us(mem.delivery_p50));
    println!("  • OneBarrier MARGINAL durability, in-fabric/mem tier   p50 = {:.2} µs  (rides the commit barrier)", us(mem.durable_p50));
    println!("  • OneBarrier MARGINAL durability, serial fsync tier    p50 = {:.2} µs  (stacks on critical path)", us(fs.durable_p50));
    let overhead = if mem.delivery_p50 > 0 {
        100.0 * mem.durable_p50 as f64 / mem.delivery_p50 as f64
    } else { 0.0 };
    println!("  → in-fabric FT marginal cost = {:.2}% of the fabric delivery latency (≈ free)", overhead);
    println!("  → fsync tier adds {:.2} µs on top (the out-of-regime durability tier)", us(fs.durable_p50));

    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}
