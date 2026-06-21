//! `ob-order` — RQ7: establishing total order, central sequencer (LLFT/NOPaxos
//! style) vs fabric/timestamp (1Pipe / OneBarrier). The sequencer serializes all
//! producers through one lock and bottlenecks as they grow; the lock-free
//! timestamp approach scales. A software model of the ordering mechanism — the
//! reason OneBarrier inherits no central-sequencer cost (cf. 1Pipe Fig 8).

use onebarrier::bench::bench_ordering;

fn main() {
    let ops = 500_000u64;
    println!("OneBarrier RQ7 — establishing total order: central sequencer vs fabric/timestamp");
    println!("  {ops} ops/producer (model of the ordering mechanism, not the full system)\n");
    println!("{:>9} {:>22} {:>22} {:>9}", "producers", "sequencer ops/s", "fabric/ts ops/s", "speedup");
    println!("{}", "-".repeat(66));
    for threads in [1usize, 2, 4, 8, 16] {
        let (seq, ts) = bench_ordering(threads, ops);
        let speedup = ts.ops_per_sec / seq.ops_per_sec;
        println!("{:>9} {:>22.0} {:>22.0} {:>8.1}x", threads, seq.ops_per_sec, ts.ops_per_sec, speedup);
    }
    println!("\nRead-out: the central sequencer's throughput stalls/degrades under producer");
    println!("contention; fabric/timestamp ordering scales — why OneBarrier pays no");
    println!("sequencer cost (1Pipe establishes the order in-network).");
}
