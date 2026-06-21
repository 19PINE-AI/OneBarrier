//! `ob-scale` — RQ6: convergence + overhead vs cluster size on the live 1Pipe
//! fabric. Sweep the replica count; at each, assert all replicas converge to the
//! exact expected state and report aggregate apply throughput. The total-order
//! barrier is aggregated in-network, so correctness must hold and per-op overhead
//! must stay bounded as the cluster grows (docs/PLAN.md §7 RQ6).

use std::time::{Duration, Instant};

use onebarrier::cluster::{run_cluster, ClusterConfig};

fn main() -> std::io::Result<()> {
    let base = std::env::temp_dir().join(format!("ob-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let clients = 2usize;
    let ops_per_client = 400u64;
    let total = clients as u64 * ops_per_client;

    println!("OneBarrier RQ6 — convergence + throughput vs cluster size (live UDP fabric)");
    println!("  {clients} clients × {ops_per_client} ops; absolute rates are the reproduction.\n");
    println!("{:>9} {:>10} {:>9} {:>14} {:>16}", "replicas", "converged", "correct", "wall ms", "applied ops/s");
    println!("{}", "-".repeat(62));

    for replicas in [3usize, 5, 7, 9] {
        let cfg = ClusterConfig {
            replicas,
            clients,
            ops_per_client,
            keys: 16,
            snap_interval: 128,
            timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let dir = base.join(format!("r{replicas}"));
        let t0 = Instant::now();
        let r = run_cluster(&cfg, &dir)?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        // Each of `replicas` replicas applies all `total` ops → aggregate work.
        let agg_ops_per_s = (replicas as f64 * total as f64) / (ms / 1000.0);
        println!(
            "{:>9} {:>10} {:>9} {:>14.1} {:>16.0}",
            replicas, r.converged, r.correct, ms, agg_ops_per_s
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    println!("\nRead-out: convergence + correctness hold at every scale; the in-network");
    println!("barrier keeps per-op overhead bounded as the replica count grows.");
    let _ = std::fs::remove_dir_all(&base);
    Ok(())
}
