//! The replicated state machine under test: a tiny `key → i64` store with an
//! idempotent op (`Set`) and a non-idempotent one (`Incr`). The Set-vs-Incr
//! distinction is the money microbenchmark for **exactly-once output
//! suppression on replay** (docs/PLAN.md §6): a naive log-replay double-applies
//! `Incr` after a crash; OneBarrier suppresses it via the per-client high-water
//! mark that the durable snapshot carries across recovery.

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
    /// Idempotent: bucket 2 in the external-effect taxonomy.
    Set(String, i64),
    /// Non-idempotent: a duplicate replay double-counts unless suppressed.
    Incr(String, i64),
    /// Read-only.
    Get(String),
}

/// The result handed back to the caller (and, in the networked node, the value
/// that would be externalized — hence subject to output suppression on replay).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    Ok,
    Value(Option<i64>),
    /// A duplicate that was recognized and neither re-applied nor re-emitted.
    Suppressed,
}

impl Op {
    pub fn set(client: u32, seq: u64, key: &str, val: i64) -> Self {
        Self { client, seq, kind: OpKind::Set(key.to_string(), val) }
    }
    pub fn incr(client: u32, seq: u64, key: &str, delta: i64) -> Self {
        Self { client, seq, kind: OpKind::Incr(key.to_string(), delta) }
    }
    pub fn get(client: u32, seq: u64, key: &str) -> Self {
        Self { client, seq, kind: OpKind::Get(key.to_string()) }
    }

    /// Wire/log encoding: `tag(1) client(4) seq(8) keylen(2) key val(8?)`.
    pub fn encode(&self) -> Vec<u8> {
        let (tag, key, val): (u8, &str, Option<i64>) = match &self.kind {
            OpKind::Set(k, v) => (0, k, Some(*v)),
            OpKind::Incr(k, v) => (1, k, Some(*v)),
            OpKind::Get(k) => (2, k, None),
        };
        let kb = key.as_bytes();
        let mut o = Vec::with_capacity(15 + kb.len() + 8);
        o.push(tag);
        o.extend_from_slice(&self.client.to_le_bytes());
        o.extend_from_slice(&self.seq.to_le_bytes());
        o.extend_from_slice(&(u16::try_from(kb.len()).unwrap_or(u16::MAX)).to_le_bytes());
        o.extend_from_slice(kb);
        if let Some(v) = val {
            o.extend_from_slice(&v.to_le_bytes());
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
            0 => OpKind::Set(key, r.i64()?),
            1 => OpKind::Incr(key, r.i64()?),
            2 => OpKind::Get(key),
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

/// The tiny KV store.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvStore {
    map: BTreeMap<String, i64>,
}

impl KvStore {
    pub fn get(&self, key: &str) -> Option<i64> {
        self.map.get(key).copied()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl StateMachine for KvStore {
    fn apply(&mut self, op: &Op) -> Output {
        match &op.kind {
            OpKind::Set(k, v) => {
                self.map.insert(k.clone(), *v);
                Output::Ok
            }
            OpKind::Incr(k, d) => {
                let e = self.map.entry(k.clone()).or_insert(0);
                *e += *d;
                Output::Value(Some(*e))
            }
            OpKind::Get(k) => Output::Value(self.map.get(k).copied()),
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(&(u32::try_from(self.map.len()).unwrap_or(u32::MAX)).to_le_bytes());
        for (k, v) in &self.map {
            let kb = k.as_bytes();
            o.extend_from_slice(&(u16::try_from(kb.len()).unwrap_or(u16::MAX)).to_le_bytes());
            o.extend_from_slice(kb);
            o.extend_from_slice(&v.to_le_bytes());
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
                let Some(v) = r.i64() else { break };
                if let Ok(k) = String::from_utf8(kb.to_vec()) {
                    map.insert(k, v);
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
