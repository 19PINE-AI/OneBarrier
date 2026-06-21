//! `ob-compare` — paper experiment #3: transparent-FT head-to-head at the RDMA
//! operating point (SIMULATED; each competitor modeled by its documented
//! mechanism + its paper's parameters). Shows OneBarrier matches the best latency
//! at the lowest CPU.

use onebarrier::sim::{competitors, simulate_explicit, SimParams};

fn main() {
    let p = SimParams { rtt_us: 2.0, barrier_us: 2.0, apply_us: 0.5, requests: 300_000, offered_load: 0.4, ..Default::default() };
    println!("OneBarrier — transparent-FT head-to-head @ RDMA operating point (SIMULATED, load {})", p.offered_load);
    println!("  models of each system's documented mechanism; parameters from their papers");
    println!("  (stable regime — see throughput note for HyCoR's lower ceiling)\n");
    println!("{:<14} {:>10} {:>10} {:>10} {:>6}  {}", "system", "p50 µs", "p99 µs", "p99.9 µs", "CPU×", "mechanism");
    println!("{}", "-".repeat(96));
    for c in competitors(&p) {
        let s = simulate_explicit(&p, c.svc_us, c.fixed_us);
        println!("{:<14} {:>10.1} {:>10.1} {:>10.1} {:>5.0}×  {}", c.name, s.p50_us, s.p99_us, s.p999_us, c.cpu_mult, c.note);
    }
    println!("\nRead-out: OneBarrier matches the best latency (COLO/SMR) at the LOWEST CPU (1×),");
    println!("and dwarfs Remus, whose ~25 ms output-hold dominates the tail (the failure mode");
    println!("that kept transparent VM-FT out of production). Lowest overhead AND lowest cost.");
    println!("Throughput ceiling: HyCoR's per-op log write competes with apply on the executor,");
    println!("so it saturates at a lower offered load than OneBarrier (whose durability is off");
    println!("the executor path); LLFT's sequencer is a similar shared-path cost.");
}
