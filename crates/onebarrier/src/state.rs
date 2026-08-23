//! The replicated state machine under test: a `key → bytes` store with Redis
//! semantics — `Set` (idempotent), `Incr` (non-idempotent, parses the value as
//! an integer), `Del`, `Get`. `Set`/`Incr` is the money microbenchmark for
//! exactly-once output suppression on replay (docs/research/PLAN.md §6): a naive
//! log-replay double-applies `Incr` after a crash; OneBarrier suppresses it via
//! the per-client high-water mark carried in the durable snapshot.

use std::collections::BTreeMap;

/// A client request. `(client, seq)` is the dedup key; `seq` is per-client and
/// strictly increasing, starting at 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Op {
    pub client: u32,
    pub seq: u64,
    pub kind: OpKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// Idempotent write of an opaque value (bucket 2 in the effect taxonomy).
    Set(String, Vec<u8>),
    /// Non-idempotent: a duplicate replay double-counts unless suppressed.
    Incr(String, i64),
    /// Delete a key (idempotent).
    Del(String),
    /// Read-only.
    Get(String),
    /// Atomic multi-key write (the database/transaction primitive): a list of
    /// `(key, Some(value)=set | None=delete)` applied all-or-nothing as one
    /// totally-ordered, durably-logged unit — so it commits and recovers atomically.
    Txn(Vec<(String, Option<Vec<u8>>)>),
}

/// The result handed back to the caller (and, in the server, the value that
/// would be externalized — hence subject to output suppression on replay).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Ok,
    /// Integer result (INCR's new value, DEL's count).
    Value(Option<i64>),
    /// Bytes result (GET).
    Bytes(Option<Vec<u8>>),
    /// A duplicate that was recognized and neither re-applied nor re-emitted.
    Suppressed,
}

impl Op {
    /// Set an integer value (stored as text, like Redis).
    pub fn set(client: u32, seq: u64, key: &str, val: i64) -> Self {
        Self::set_bytes(client, seq, key, val.to_string().as_bytes())
    }
    pub fn set_bytes(client: u32, seq: u64, key: &str, val: &[u8]) -> Self {
        Self { client, seq, kind: OpKind::Set(key.to_string(), val.to_vec()) }
    }
    pub fn incr(client: u32, seq: u64, key: &str, delta: i64) -> Self {
        Self { client, seq, kind: OpKind::Incr(key.to_string(), delta) }
    }
    pub fn del(client: u32, seq: u64, key: &str) -> Self {
        Self { client, seq, kind: OpKind::Del(key.to_string()) }
    }
    pub fn get(client: u32, seq: u64, key: &str) -> Self {
        Self { client, seq, kind: OpKind::Get(key.to_string()) }
    }
    pub fn txn(client: u32, seq: u64, writes: Vec<(String, Option<Vec<u8>>)>) -> Self {
        Self { client, seq, kind: OpKind::Txn(writes) }
    }

    /// Wire/log encoding: `tag(1) client(4) seq(8) keylen(2) key [vallen(4) val | delta(8)]`.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        let (tag, key): (u8, &str) = match &self.kind {
            OpKind::Set(k, _) => (0, k),
            OpKind::Incr(k, _) => (1, k),
            OpKind::Get(k) => (2, k),
            OpKind::Del(k) => (3, k),
            OpKind::Txn(_) => (4, ""),
        };
        let kb = key.as_bytes();
        o.push(tag);
        o.extend_from_slice(&self.client.to_le_bytes());
        o.extend_from_slice(&self.seq.to_le_bytes());
        o.extend_from_slice(&(u16::try_from(kb.len()).unwrap_or(u16::MAX)).to_le_bytes());
        o.extend_from_slice(kb);
        match &self.kind {
            OpKind::Set(_, v) => {
                o.extend_from_slice(&(u32::try_from(v.len()).unwrap_or(u32::MAX)).to_le_bytes());
                o.extend_from_slice(v);
            }
            OpKind::Incr(_, d) => o.extend_from_slice(&d.to_le_bytes()),
            OpKind::Get(_) | OpKind::Del(_) => {}
            OpKind::Txn(writes) => {
                o.extend_from_slice(&(u16::try_from(writes.len()).unwrap_or(u16::MAX)).to_le_bytes());
                for (k, v) in writes {
                    let kb = k.as_bytes();
                    o.extend_from_slice(&(u16::try_from(kb.len()).unwrap_or(u16::MAX)).to_le_bytes());
                    o.extend_from_slice(kb);
                    match v {
                        Some(val) => {
                            o.push(1);
                            o.extend_from_slice(&(u32::try_from(val.len()).unwrap_or(u32::MAX)).to_le_bytes());
                            o.extend_from_slice(val);
                        }
                        None => o.push(0),
                    }
                }
            }
        }
        o
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        let tag = r.u8()?;
        let client = r.u32()?;
        let seq = r.u64()?;
        let klen = r.u16()? as usize;
        let key = String::from_utf8(r.bytes(klen)?.to_vec()).ok()?;
        let kind = match tag {
            0 => {
                let vlen = r.u32()? as usize;
                OpKind::Set(key, r.bytes(vlen)?.to_vec())
            }
            1 => OpKind::Incr(key, r.i64()?),
            2 => OpKind::Get(key),
            3 => OpKind::Del(key),
            4 => {
                let n = r.u16()? as usize;
                let mut writes = Vec::with_capacity(n);
                for _ in 0..n {
                    let kl = r.u16()? as usize;
                    let k = String::from_utf8(r.bytes(kl)?.to_vec()).ok()?;
                    let v = match r.u8()? {
                        1 => {
                            let vl = r.u32()? as usize;
                            Some(r.bytes(vl)?.to_vec())
                        }
                        _ => None,
                    };
                    writes.push((k, v));
                }
                OpKind::Txn(writes)
            }
            _ => return None,
        };
        Some(Self { client, seq, kind })
    }
}

/// A deterministic state machine OneBarrier replicates by replaying the
/// totally-ordered op stream. `apply` must be a pure function of the op and
/// prior state (local non-determinism is virtualized upstream).
pub trait StateMachine: Default {
    fn apply(&mut self, op: &Op) -> Output;
    fn snapshot(&self) -> Vec<u8>;
    fn restore(bytes: &[u8]) -> Self;
}

/// The `key → bytes` store with Redis semantics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvStore {
    map: BTreeMap<String, Vec<u8>>,
}

impl KvStore {
    pub fn get_bytes(&self, key: &str) -> Option<&[u8]> {
        self.map.get(key).map(Vec::as_slice)
    }
    /// Parse a key's value as an integer (0 / None semantics for INCR-style use).
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.map.get(key).and_then(|v| std::str::from_utf8(v).ok()?.trim().parse().ok())
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    /// Integer view of the store, sorted — for cross-replica convergence checks
    /// over integer (INCR) workloads.
    pub fn entries(&self) -> Vec<(String, i64)> {
        self.map
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), std::str::from_utf8(v).ok()?.trim().parse().ok()?)))
            .collect()
    }
}

impl StateMachine for KvStore {
    fn apply(&mut self, op: &Op) -> Output {
        match &op.kind {
            OpKind::Set(k, v) => {
                self.map.insert(k.clone(), v.clone());
                Output::Ok
            }
            OpKind::Incr(k, d) => {
                let cur: i64 = self
                    .map
                    .get(k)
                    .and_then(|v| std::str::from_utf8(v).ok()?.trim().parse().ok())
                    .unwrap_or(0);
                let next = cur + *d;
                self.map.insert(k.clone(), next.to_string().into_bytes());
                Output::Value(Some(next))
            }
            OpKind::Del(k) => Output::Value(Some(i64::from(self.map.remove(k).is_some()))),
            OpKind::Get(k) => Output::Bytes(self.map.get(k).cloned()),
            OpKind::Txn(writes) => {
                // Atomic all-or-nothing: one deliver = one durable record = applied
                // and recovered together.
                for (k, v) in writes {
                    match v {
                        Some(val) => {
                            self.map.insert(k.clone(), val.clone());
                        }
                        None => {
                            self.map.remove(k);
                        }
                    }
                }
                Output::Value(Some(writes.len() as i64))
            }
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(&(u32::try_from(self.map.len()).unwrap_or(u32::MAX)).to_le_bytes());
        for (k, v) in &self.map {
            let kb = k.as_bytes();
            o.extend_from_slice(&(u16::try_from(kb.len()).unwrap_or(u16::MAX)).to_le_bytes());
            o.extend_from_slice(kb);
            o.extend_from_slice(&(u32::try_from(v.len()).unwrap_or(u32::MAX)).to_le_bytes());
            o.extend_from_slice(v);
        }
        o
    }

    fn restore(bytes: &[u8]) -> Self {
        let mut map = BTreeMap::new();
        let mut r = Reader::new(bytes);
        if let Some(n) = r.u32() {
            for _ in 0..n {
                let Some(klen) = r.u16() else { break };
                let Some(kb) = r.bytes(klen as usize) else { break };
                let Some(vlen) = r.u32() else { break };
                let Some(vb) = r.bytes(vlen as usize) else { break };
                if let Ok(k) = String::from_utf8(kb.to_vec()) {
                    map.insert(k, vb.to_vec());
                }
            }
        }
        Self { map }
    }
}

/// Little-endian byte reader with bounds checks (returns `None` past the end).
pub(crate) struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    pub(crate) fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    pub(crate) fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.bytes(8)?.try_into().ok()?))
    }
    pub(crate) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
}
