//! A from-scratch **linearizability checker** (Wing & Gong / Lowe algorithm) for a
//! register/counter history — paper exp #7, turning the `ob-jepsen` acked-set
//! check into a real linearizability oracle. A key-value store is linearizable iff
//! each key (an independent register) is linearizable (locality), so this register
//! checker is the core. NP-hard in general; tractable with memoization for the
//! histories a fault-injection run produces.

use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Write(i64),
    Read(i64),
}

/// One operation with its real-time interval `[inv, res]`.
#[derive(Clone, Copy, Debug)]
pub struct LinOp {
    pub proc_id: u32,
    pub action: Action,
    pub inv: u64,
    pub res: u64,
}

/// Is the history linearizable against a register starting at `init`?
/// Real-time order is honored (`res(A) < inv(B)` ⇒ A before B); a `Read(v)` is
/// valid iff the register's value at its linearization point is `v`.
pub fn is_linearizable(ops: &[LinOp], init: i64) -> bool {
    // Time-ordered call/return entries, with stable indices into `ops`.
    let mut entries: Vec<(u64, bool, usize)> = Vec::with_capacity(ops.len() * 2);
    for (i, op) in ops.iter().enumerate() {
        entries.push((op.inv, true, i)); // call
        entries.push((op.res, false, i)); // return
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1))); // calls before returns at ties

    let remaining: Vec<usize> = (0..ops.len()).collect();
    let mut memo: HashSet<(Vec<usize>, i64)> = HashSet::new();
    recurse(ops, &entries, &remaining, init, &mut memo)
}

fn valid(action: Action, value: i64) -> bool {
    match action {
        Action::Write(_) => true,
        Action::Read(v) => v == value,
    }
}
fn apply(action: Action, value: i64) -> i64 {
    match action {
        Action::Write(v) => v,
        Action::Read(_) => value,
    }
}

fn recurse(
    ops: &[LinOp],
    entries: &[(u64, bool, usize)],
    remaining: &[usize],
    value: i64,
    memo: &mut HashSet<(Vec<usize>, i64)>,
) -> bool {
    if remaining.is_empty() {
        return true;
    }
    let key = (remaining.to_vec(), value);
    if memo.contains(&key) {
        return false; // this (remaining, state) was explored and failed
    }

    // Candidates = ops whose CALL appears before the FIRST RETURN among the
    // remaining entries (an op that returns before another is called must be
    // linearized first — the real-time constraint).
    let rem_set: HashSet<usize> = remaining.iter().copied().collect();
    let mut candidates: Vec<usize> = Vec::new();
    for &(_, is_call, id) in entries {
        if !rem_set.contains(&id) {
            continue;
        }
        if is_call {
            candidates.push(id);
        } else {
            break; // first return among remaining: stop collecting calls
        }
    }

    for &id in &candidates {
        let act = ops[id].action;
        if !valid(act, value) {
            continue;
        }
        let next_val = apply(act, value);
        let next_remaining: Vec<usize> = remaining.iter().copied().filter(|&x| x != id).collect();
        if recurse(ops, entries, &next_remaining, next_val, memo) {
            return true;
        }
    }

    memo.insert(key);
    false
}

#[cfg(test)]
mod tests {
    use super::Action::{Read, Write};
    use super::*;

    fn op(p: u32, a: Action, inv: u64, res: u64) -> LinOp {
        LinOp { proc_id: p, action: a, inv, res }
    }

    #[test]
    fn sequential_write_then_read_is_linearizable() {
        let h = vec![op(1, Write(1), 0, 2), op(2, Read(1), 3, 4)];
        assert!(is_linearizable(&h, 0));
    }

    #[test]
    fn stale_read_after_write_returns_is_not_linearizable() {
        // W(1) returns at t=1; R(0) is called at t=2 — must see 1, not 0.
        let h = vec![op(1, Write(1), 0, 1), op(2, Read(0), 2, 3)];
        assert!(!is_linearizable(&h, 0));
    }

    #[test]
    fn concurrent_read_during_write_is_linearizable() {
        // R(1) overlaps W(1): linearize W before R inside the overlap.
        let h = vec![op(1, Write(1), 0, 3), op(2, Read(1), 1, 2)];
        assert!(is_linearizable(&h, 0));
        // R(0) overlapping is ALSO fine (linearize R before W).
        let h2 = vec![op(1, Write(1), 0, 3), op(2, Read(0), 1, 2)];
        assert!(is_linearizable(&h2, 0));
    }

    #[test]
    fn lost_update_counter_history_is_not_linearizable() {
        // Two writers and a reader; reader sees a value never written in a valid
        // order (real-time forces 1 then 2, but read returns 1 after 2 committed).
        let h = vec![
            op(1, Write(1), 0, 1),
            op(2, Write(2), 2, 3),
            op(3, Read(1), 4, 5), // after W(2) returns — must see 2
        ];
        assert!(!is_linearizable(&h, 0));
    }

    #[test]
    fn larger_concurrent_linearizable_history() {
        let h = vec![
            op(1, Write(1), 0, 5),
            op(2, Read(1), 2, 4),
            op(3, Write(2), 4, 9),
            op(2, Read(2), 6, 8),
            op(1, Read(2), 10, 11),
        ];
        assert!(is_linearizable(&h, 0));
    }
}
