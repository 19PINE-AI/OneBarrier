//! OneBarrier core — the deterministic-replay engine.
//!
//! Transparent passive fault tolerance as a byproduct of total-order
//! communication. This crate is the M0 core (docs/PLAN.md §8): it consumes a
//! **totally-ordered** op stream (in the networked node, the `Delivered` stream
//! from `1pipe-net::ReliableHost`), applies it to a deterministic
//! [`StateMachine`] with **exactly-once** semantics, persists a durable ordered
//! log + timestamp-T snapshots, and recovers by restoring the latest snapshot
//! and replaying the log forward — *without a message-order log*, because the
//! fabric already supplies the order.
//!
//! The output-suppression high-water mark (per client) is carried inside the
//! snapshot, so a duplicate op replayed after recovery is recognized and neither
//! re-applied nor re-externalized — the Set-vs-Incr correctness result (RQ4).

pub mod bench;
pub mod cluster;
pub mod durable;
pub mod state;

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use onepipe_core::timestamp::Timestamp;

pub use state::{KvStore, Op, OpKind, Output, StateMachine};
use durable::Durable;
use state::Reader;

/// Counters for experiments and assertions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Ops applied for the first time (state-changing).
    pub applied: u64,
    /// Duplicates recognized and suppressed (exactly-once).
    pub suppressed: u64,
    /// Snapshots taken.
    pub snapshots: u64,
    /// Records re-applied during the last recovery.
    pub replayed: u64,
}

/// The OneBarrier replication engine over a deterministic [`StateMachine`].
#[derive(Debug)]
pub struct Engine<S: StateMachine> {
    state: S,
    /// Per-client highest `seq` applied — the exactly-once / output-suppression
    /// high-water mark. Persisted in every snapshot.
    applied: BTreeMap<u32, u64>,
    /// Highest applied logical timestamp (the snapshot horizon when we cut).
    last_ts: u64,
    dur: Durable,
    snap_interval: u64,
    ops_since_snap: u64,
    pub stats: Stats,
    dir: PathBuf,
}

impl<S: StateMachine> Engine<S> {
    /// Start a **fresh** engine rooted at `dir`. Snapshots every
    /// `snap_interval` applied ops (the RQ8 interval knob); `fsync` selects the
    /// stable-storage durability tier.
    pub fn create(dir: impl AsRef<Path>, snap_interval: u64, fsync: bool) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let dur = Durable::open(&dir, fsync)?;
        Ok(Self {
            state: S::default(),
            applied: BTreeMap::new(),
            last_ts: 0,
            dur,
            snap_interval: snap_interval.max(1),
            ops_since_snap: 0,
            stats: Stats::default(),
            dir,
        })
    }

    /// Recover an engine from the durable store at `dir`: load the latest
    /// snapshot, then replay the log forward. Recovery is idempotent — the
    /// per-client high-water mark suppresses any already-applied replayed op.
    pub fn recover(dir: impl AsRef<Path>, snap_interval: u64, fsync: bool) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let dur = Durable::open(&dir, fsync)?;
        let mut eng = Self {
            state: S::default(),
            applied: BTreeMap::new(),
            last_ts: 0,
            dur,
            snap_interval: snap_interval.max(1),
            ops_since_snap: 0,
            stats: Stats::default(),
            dir,
        };
        if let Some(snap) = eng.dur.read_snapshot()? {
            eng.load_snapshot(&snap);
        }
        let mut replayed = 0u64;
        for (ts, op_bytes) in eng.dur.read_log()? {
            if let Some(op) = Op::decode(&op_bytes) {
                if eng.apply_dedup(ts, &op).is_some() {
                    replayed += 1;
                }
            }
        }
        eng.stats.replayed = replayed;
        // Account replayed ops toward the next snapshot boundary.
        eng.ops_since_snap = replayed % eng.snap_interval;
        Ok(eng)
    }

    /// The live delivery path: apply one totally-ordered op, persist it, and
    /// maybe snapshot. Returns [`Output::Suppressed`] for a duplicate (no
    /// re-apply, no re-emit, no log append).
    pub fn deliver(&mut self, ts: Timestamp, op: &Op) -> io::Result<Output> {
        let ts_ns = ts.as_nanos();
        match self.apply_dedup(ts_ns, op) {
            None => {
                self.stats.suppressed += 1;
                Ok(Output::Suppressed)
            }
            Some(out) => {
                self.stats.applied += 1;
                self.dur.append(ts_ns, &op.encode())?;
                self.ops_since_snap += 1;
                if self.ops_since_snap >= self.snap_interval {
                    self.snapshot()?;
                }
                Ok(out)
            }
        }
    }

    /// Apply with exactly-once dedup, **without** touching the durable log
    /// (shared by the live path and replay). `None` = duplicate (suppressed).
    fn apply_dedup(&mut self, ts_ns: u64, op: &Op) -> Option<Output> {
        let hw = self.applied.get(&op.client).copied().unwrap_or(0);
        if op.seq <= hw {
            return None; // already applied — suppress
        }
        let out = self.state.apply(op);
        self.applied.insert(op.client, op.seq);
        if ts_ns > self.last_ts {
            self.last_ts = ts_ns;
        }
        Some(out)
    }

    /// Cut a timestamp-T snapshot (state + high-water marks + horizon).
    pub fn snapshot(&mut self) -> io::Result<()> {
        let bytes = self.encode_snapshot();
        self.dur.write_snapshot(&bytes)?;
        self.ops_since_snap = 0;
        self.stats.snapshots += 1;
        Ok(())
    }

    pub fn state(&self) -> &S {
        &self.state
    }
    pub fn last_ts(&self) -> u64 {
        self.last_ts
    }

    /// Export this engine's full state for a **state transfer** to a recovering
    /// peer (snapshot bytes that, in deployment, travel over the fabric). Carries
    /// the per-client high-water marks so exactly-once survives the transfer.
    pub fn export_state(&self) -> Vec<u8> {
        self.encode_snapshot()
    }

    /// Install a peer's exported state into this (recovering) engine: adopt its
    /// state + high-water marks + horizon, persist it durably, and drop the now
    /// superseded log. Used to catch a crash-recovered replica up to the live
    /// cut after it has restored its own (stale) durable prefix.
    pub fn import_state(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.load_snapshot(bytes);
        self.dur.write_snapshot(bytes)?;
        self.ops_since_snap = 0;
        Ok(())
    }
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn encode_snapshot(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(&self.last_ts.to_le_bytes());
        o.extend_from_slice(&(u32::try_from(self.applied.len()).unwrap_or(u32::MAX)).to_le_bytes());
        for (c, s) in &self.applied {
            o.extend_from_slice(&c.to_le_bytes());
            o.extend_from_slice(&s.to_le_bytes());
        }
        let sm = self.state.snapshot();
        o.extend_from_slice(&(u32::try_from(sm.len()).unwrap_or(u32::MAX)).to_le_bytes());
        o.extend_from_slice(&sm);
        o
    }

    fn load_snapshot(&mut self, bytes: &[u8]) {
        let mut r = Reader::new(bytes);
        let Some(last_ts) = r.u64() else { return };
        let Some(n) = r.u32() else { return };
        let mut applied = BTreeMap::new();
        for _ in 0..n {
            let (Some(c), Some(s)) = (r.u32(), r.u64()) else { break };
            applied.insert(c, s);
        }
        let Some(smlen) = r.u32() else { return };
        let Some(sm) = r.bytes(smlen as usize) else { return };
        self.state = S::restore(sm);
        self.applied = applied;
        self.last_ts = last_ts;
    }
}

/// Reference oracle for tests/RQ4: apply a totally-ordered op stream to a fresh
/// state machine with the same exactly-once dedup, with no persistence. A
/// correct engine's post-recovery state must equal this.
pub fn reference_apply<S: StateMachine>(ops: &[Op]) -> S {
    let mut s = S::default();
    let mut applied: BTreeMap<u32, u64> = BTreeMap::new();
    for op in ops {
        let hw = applied.get(&op.client).copied().unwrap_or(0);
        if op.seq > hw {
            s.apply(op);
            applied.insert(op.client, op.seq);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        // Unique, dependency-free temp dir (no Math.random / tempfile crate).
        static CTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = CTR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("onebarrier-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn ts(n: u64) -> Timestamp {
        Timestamp::from_nanos(n)
    }

    #[test]
    fn recovery_reconstructs_identical_state() {
        let dir = tmpdir("recover");
        let ops = vec![
            Op::set(1, 1, "a", 10),
            Op::incr(1, 2, "a", 5),
            Op::set(2, 1, "b", 100),
            Op::incr(2, 2, "b", -1),
            Op::incr(1, 3, "a", 1),
        ];
        {
            let mut e = Engine::<KvStore>::create(&dir, 2, false).unwrap();
            for (i, op) in ops.iter().enumerate() {
                e.deliver(ts((i as u64 + 1) * 10), op).unwrap();
            }
            assert!(e.stats.snapshots >= 1, "expected at least one snapshot");
        } // drop = "crash"

        let e = Engine::<KvStore>::recover(&dir, 2, false).unwrap();
        let reference = reference_apply::<KvStore>(&ops);
        assert_eq!(e.state(), &reference, "post-recovery state must equal serial reference");
        assert_eq!(e.state().get("a"), Some(16));
        assert_eq!(e.state().get("b"), Some(99));
    }

    #[test]
    fn incr_not_double_applied_after_crash() {
        // The money microbenchmark: a non-idempotent INCR must NOT be re-applied
        // when its message is re-delivered after recovery.
        let dir = tmpdir("incr");
        {
            let mut e = Engine::<KvStore>::create(&dir, 100, false).unwrap();
            assert_eq!(e.deliver(ts(10), &Op::incr(1, 1, "x", 5)).unwrap(), Output::Value(Some(5)));
        } // crash before snapshot

        let mut e = Engine::<KvStore>::recover(&dir, 100, false).unwrap();
        assert_eq!(e.state().get("x"), Some(5), "replay reconstructs x=5, not 10");
        // The fabric re-delivers the same op (same client/seq) post-recovery:
        assert_eq!(e.deliver(ts(10), &Op::incr(1, 1, "x", 5)).unwrap(), Output::Suppressed);
        assert_eq!(e.state().get("x"), Some(5), "duplicate INCR suppressed — not double-counted");
        assert_eq!(e.stats.suppressed, 1);
    }

    #[test]
    fn idempotent_set_duplicate_is_suppressed() {
        let dir = tmpdir("set");
        let mut e = Engine::<KvStore>::create(&dir, 100, false).unwrap();
        assert_eq!(e.deliver(ts(10), &Op::set(7, 1, "k", 42)).unwrap(), Output::Ok);
        assert_eq!(e.deliver(ts(20), &Op::set(7, 1, "k", 42)).unwrap(), Output::Suppressed);
        assert_eq!(e.state().get("k"), Some(42));
    }

    #[test]
    fn op_encode_decode_roundtrips() {
        for op in [
            Op::set(3, 9, "hello", -17),
            Op::incr(0, 1, "", i64::MIN),
            Op::get(u32::MAX, u64::MAX, "a key with spaces"),
        ] {
            assert_eq!(Op::decode(&op.encode()), Some(op));
        }
    }

    #[test]
    fn crash_recover_then_state_transfer_catches_up() {
        // RQ3/RQ4 (engine level, deterministic): a replica crashes mid-stream,
        // recovers its consistent pre-crash prefix from its own durable store,
        // then catches up to the live cut via a state transfer from a survivor —
        // and that caught-up state is itself durable across a second crash.
        let dir_a = tmpdir("xfer-a");
        let dir_b = tmpdir("xfer-b");
        let ops: Vec<Op> =
            (0..20).map(|i| Op::incr(1, i + 1, &format!("k{}", i % 4), 1)).collect();

        // Survivor A applies the whole stream.
        let mut a = Engine::<KvStore>::create(&dir_a, 8, false).unwrap();
        for (i, op) in ops.iter().enumerate() {
            a.deliver(ts((i as u64 + 1) * 10), op).unwrap();
        }

        // Victim B applies the first 12 ops, then crashes.
        {
            let mut b = Engine::<KvStore>::create(&dir_b, 8, false).unwrap();
            for (i, op) in ops.iter().take(12).enumerate() {
                b.deliver(ts((i as u64 + 1) * 10), op).unwrap();
            }
        }

        // B recovers — to a *consistent prefix*, not corruption.
        let mut b = Engine::<KvStore>::recover(&dir_b, 8, false).unwrap();
        assert_eq!(
            b.state(),
            &reference_apply::<KvStore>(&ops[..12]),
            "B recovered to its consistent pre-crash prefix"
        );

        // Catch up to the live cut via state transfer from the survivor.
        b.import_state(&a.export_state()).unwrap();
        assert_eq!(b.state(), a.state(), "B caught up to A via state transfer");
        assert_eq!(b.state(), &reference_apply::<KvStore>(&ops));

        // The transferred state is durable: B survives a second crash.
        drop(b);
        let b2 = Engine::<KvStore>::recover(&dir_b, 8, false).unwrap();
        assert_eq!(b2.state(), a.state(), "transferred state durable across a 2nd crash");
    }

    #[test]
    fn crash_between_snapshot_and_log_clear_is_idempotent() {
        // Recovery over a snapshot whose covered ops also still sit in the log
        // must not double-apply (dedup makes the leftover records suppressed).
        let dir = tmpdir("crashwin");
        let ops = vec![Op::incr(1, 1, "n", 1), Op::incr(1, 2, "n", 1), Op::incr(1, 3, "n", 1)];
        {
            let mut e = Engine::<KvStore>::create(&dir, 100, false).unwrap();
            for (i, op) in ops.iter().enumerate() {
                e.deliver(ts((i as u64 + 1) * 10), op).unwrap();
            }
            e.snapshot().unwrap(); // snapshot installed; in a real crash the log
                                   // might still hold these records — simulate by
                                   // re-appending them below.
            for (i, op) in ops.iter().enumerate() {
                e.dur.append(ts((i as u64 + 1) * 10).as_nanos(), &op.encode()).unwrap();
            }
        }
        let e = Engine::<KvStore>::recover(&dir, 100, false).unwrap();
        assert_eq!(e.state().get("n"), Some(3), "idempotent recovery: n=3, not 6");
    }
}
