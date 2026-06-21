//! `ob-demo` — a self-contained, in-process demonstration of the OneBarrier core:
//! apply a totally-ordered op stream, snapshot, simulate a crash, recover by
//! replaying the durable log, and show that (a) state is reconstructed exactly
//! and (b) a re-delivered non-idempotent `INCR` is suppressed (exactly-once).
//!
//! This is the M0 milestone running end-to-end. M1 wires the same `Engine` to
//! `1pipe-net::ReliableHost` so the op stream is the fabric's `Delivered` order.

use onebarrier::{Engine, KvStore, Op, Output};
use onepipe_core::timestamp::Timestamp;

fn ts(n: u64) -> Timestamp {
    Timestamp::from_nanos(n)
}

fn main() -> std::io::Result<()> {
    let dir = std::env::temp_dir().join(format!("ob-demo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    println!("OneBarrier M0 demo — durable snapshot+replay over a total order");
    println!("store dir: {}\n", dir.display());

    // --- live run: a mixed workload from two clients, snapshot every 3 ops ---
    let ops = vec![
        Op::set(1, 1, "balance", 100),
        Op::incr(1, 2, "balance", -30),
        Op::set(2, 1, "name_len", 7),
        Op::incr(1, 3, "balance", 50),
        Op::incr(2, 2, "name_len", 1),
    ];
    {
        let mut e = Engine::<KvStore>::create(&dir, 3, false)?;
        for (i, op) in ops.iter().enumerate() {
            let out = e.deliver(ts((i as u64 + 1) * 10), op)?;
            println!("  applied {op:?} -> {out:?}");
        }
        println!(
            "\nbefore crash: balance={:?} name_len={:?}  (applied={}, snapshots={})",
            e.state().get("balance"),
            e.state().get("name_len"),
            e.stats.applied,
            e.stats.snapshots
        );
    } // drop = crash

    // --- recover ---
    println!("\n*** CRASH — recovering from durable store ***\n");
    let mut e = Engine::<KvStore>::recover(&dir, 3, false)?;
    println!(
        "after recovery: balance={:?} name_len={:?}  (replayed {} records from log)",
        e.state().get("balance"),
        e.state().get("name_len"),
        e.stats.replayed
    );
    assert_eq!(e.state().get("balance"), Some(120));
    assert_eq!(e.state().get("name_len"), Some(8));

    // --- the fabric re-delivers an in-flight op after recovery: must suppress ---
    let dup = Op::incr(1, 3, "balance", 50);
    let out = e.deliver(ts(40), &dup)?;
    println!("\nre-delivered in-flight {dup:?} -> {out:?}");
    assert_eq!(out, Output::Suppressed);
    assert_eq!(e.state().get("balance"), Some(120), "duplicate INCR not double-counted");

    println!("\nOK: state reconstructed exactly; exactly-once held across recovery.");
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
