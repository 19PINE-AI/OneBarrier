//! File-backed durable ordered log + snapshot — the **stable-storage durability
//! tier**. RQ2 (docs/research/PLAN.md §7) sweeps durability tiers; the low-latency tier
//! is in-fabric RDMA replication ridden as 1Pipe 2PC phase-1 (1 RTT, §2.2.2 of
//! the 1Pipe paper), which this same interface will front in M2. Here we provide
//! the correctness-first disk tier with an `fsync` knob so the tier's cost is
//! measurable rather than assumed.
//!
//! Crash-safety: `write_snapshot` renames a temp file into place (atomic) and
//! then clears the log. A crash between those steps leaves stale log records
//! `≤ snapshot_ts`; recovery replays them but the per-client high-water mark in
//! the snapshot makes them duplicates, so they are suppressed — recovery is
//! idempotent regardless of where the crash lands.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Durable {
    dir: PathBuf,
    log_path: PathBuf,
    snap_path: PathBuf,
    fsync: bool,
    /// Cached append handle (opened lazily). Holding it open is what makes the
    /// in-memory/no-fsync tier a single `write()` syscall per op (data in the OS
    /// page cache — survives process crash, not power loss) rather than an
    /// open+write+close. The fsync tier adds `sync_all` (stable storage).
    log: Option<File>,
}

impl Durable {
    /// Open (creating if absent) a durable store rooted at `dir`.
    pub fn open(dir: impl AsRef<Path>, fsync: bool) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self {
            log_path: dir.join("oplog"),
            snap_path: dir.join("snapshot"),
            dir,
            fsync,
            log: None,
        })
    }

    /// Append one ordered record: `ts(8) len(4) op_bytes`.
    pub fn append(&mut self, ts: u64, op_bytes: &[u8]) -> io::Result<()> {
        if self.log.is_none() {
            self.log = Some(OpenOptions::new().create(true).append(true).open(&self.log_path)?);
        }
        let f = self.log.as_mut().unwrap();
        let mut rec = Vec::with_capacity(12 + op_bytes.len());
        rec.extend_from_slice(&ts.to_le_bytes());
        rec.extend_from_slice(&(u32::try_from(op_bytes.len()).unwrap_or(u32::MAX)).to_le_bytes());
        rec.extend_from_slice(op_bytes);
        f.write_all(&rec)?;
        if self.fsync {
            f.sync_all()?;
        }
        Ok(())
    }

    /// Atomically install a snapshot, then clear the now-covered log.
    pub fn write_snapshot(&mut self, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join("snapshot.tmp");
        {
            let mut f = File::create(&tmp)?;
            f.write_all(bytes)?;
            if self.fsync {
                f.sync_all()?;
            }
        }
        fs::rename(&tmp, &self.snap_path)?;
        // Everything up to the snapshot's last_ts is now captured; clear the log.
        self.log = None; // drop the cached handle before removing the file
        let _ = fs::remove_file(&self.log_path);
        Ok(())
    }

    pub fn read_snapshot(&self) -> io::Result<Option<Vec<u8>>> {
        match File::open(&self.snap_path) {
            Ok(mut f) => {
                let mut b = Vec::new();
                f.read_to_end(&mut b)?;
                Ok(Some(b))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read every log record in order as `(ts, op_bytes)`.
    pub fn read_log(&self) -> io::Result<Vec<(u64, Vec<u8>)>> {
        let raw = match fs::read(&self.log_path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        let mut p = 0usize;
        while p + 12 <= raw.len() {
            let ts = u64::from_le_bytes(raw[p..p + 8].try_into().unwrap());
            let len = u32::from_le_bytes(raw[p + 8..p + 12].try_into().unwrap()) as usize;
            p += 12;
            // Truncated tail (torn write at crash): stop cleanly.
            if p + len > raw.len() {
                break;
            }
            out.push((ts, raw[p..p + len].to_vec()));
            p += len;
        }
        Ok(out)
    }
}
