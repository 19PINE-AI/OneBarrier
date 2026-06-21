//! M3 (Track A) — production application servers on the OneBarrier engine.
//!
//! [`KvService`] owns the `Engine<KvStore>` in a **single executor thread** and
//! applies every op in one serial order (the share-nothing / event-loop model);
//! it is shared by every protocol front-end. A [`Protocol`] turns a TCP
//! connection's wire format into engine ops and back. Writes are durable
//! (ordered log + timestamp snapshot) and the service **recovers on restart** —
//! transparent fault tolerance for real wire-protocol services. Front-ends:
//! [`crate::resp`] (Redis) here, [`crate::memcache`] (Memcached) alongside.

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use onepipe_core::timestamp::Timestamp;

use crate::{Engine, KvStore, Op, Output};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub dir: PathBuf,
    pub snap_interval: u64,
    pub fsync: bool,
}

// ---------------------------------------------------------------------------
// KvService — the shared single-executor engine front
// ---------------------------------------------------------------------------

enum Req {
    Apply(Op),
    Len,
}
enum SvcResp {
    Out(Output),
    Len(usize),
}
struct Job {
    req: Req,
    reply: SyncSender<SvcResp>,
}

/// A clonable handle to the single executor thread that owns the engine.
#[derive(Clone, Debug)]
pub struct KvService {
    tx: Sender<Job>,
}

static GLOBAL_TICK: AtomicU64 = AtomicU64::new(1);

impl KvService {
    /// Spawn the executor (recovering the durable store if present). Also returns
    /// the highest client id seen in the recovered state, so the caller can seed
    /// fresh connection ids above it (exactly-once correctness across restarts).
    pub fn start(cfg: &ServerConfig) -> io::Result<(Self, JoinHandle<io::Result<()>>, u32)> {
        let dir = cfg.dir.clone();
        let snap = cfg.snap_interval;
        let fsync = cfg.fsync;
        let (tx, rx) = mpsc::channel::<Job>();
        let (init_tx, init_rx) = mpsc::sync_channel::<u32>(1);
        let handle = thread::spawn(move || -> io::Result<()> {
            let mut engine = if dir.join("snapshot").exists() || dir.join("oplog").exists() {
                Engine::<KvStore>::recover(&dir, snap, fsync)?
            } else {
                Engine::<KvStore>::create(&dir, snap, fsync)?
            };
            let _ = init_tx.send(engine.max_client());
            let mut clock: u64 = engine.last_ts() + 1;
            while let Ok(job) = rx.recv() {
                let resp = match job.req {
                    Req::Apply(op) => {
                        clock += 1;
                        let g = GLOBAL_TICK.fetch_add(1, Ordering::Relaxed);
                        let ts = Timestamp::from_nanos(clock.max(g));
                        SvcResp::Out(engine.deliver(ts, &op)?)
                    }
                    Req::Len => SvcResp::Len(engine.state().len()),
                };
                let _ = job.reply.send(resp);
            }
            Ok(())
        });
        let max_client = init_rx.recv().unwrap_or(0);
        Ok((Self { tx }, handle, max_client))
    }

    /// Apply an op through the serial executor. `None` if the executor is gone.
    pub fn apply(&self, op: Op) -> Option<Output> {
        let (rtx, rrx) = mpsc::sync_channel(1);
        self.tx.send(Job { req: Req::Apply(op), reply: rtx }).ok()?;
        match rrx.recv().ok()? {
            SvcResp::Out(o) => Some(o),
            SvcResp::Len(_) => None,
        }
    }

    pub fn len(&self) -> Option<usize> {
        let (rtx, rrx) = mpsc::sync_channel(1);
        self.tx.send(Job { req: Req::Len, reply: rtx }).ok()?;
        match rrx.recv().ok()? {
            SvcResp::Len(n) => Some(n),
            SvcResp::Out(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Protocol trait + generic accept loop
// ---------------------------------------------------------------------------

/// A wire protocol front-end. `serve_conn` drives one connection to completion.
pub trait Protocol: Send + Sync + 'static {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()>;
}

/// A running server.
#[derive(Debug)]
pub struct ServerHandle {
    pub addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    executor: Option<JoinHandle<io::Result<()>>>,
}

impl ServerHandle {
    pub fn stop(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(a) = self.accept.take() {
            let _ = a.join();
        }
        if let Some(e) = self.executor.take() {
            let _ = e.join();
        }
    }
}

/// Start a server speaking `proto`, in the background.
pub fn start<P: Protocol>(cfg: ServerConfig, proto: Arc<P>) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(cfg.addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let (svc, executor, max_client) = KvService::start(&cfg)?;

    // Fresh connections must get client ids above any recovered one.
    let conn_id = Arc::new(AtomicU32::new(max_client + 1));
    let accept_shutdown = Arc::clone(&shutdown);
    let accept = thread::spawn(move || {
        let svc = svc; // moved here; dropped when accept loop ends → executor exits
        let mut conns: Vec<JoinHandle<()>> = Vec::new();
        for stream in listener.incoming() {
            if accept_shutdown.load(Ordering::SeqCst) {
                break;
            }
            match stream {
                Ok(s) => {
                    let id = conn_id.fetch_add(1, Ordering::Relaxed);
                    let sd = Arc::clone(&accept_shutdown);
                    let svc = svc.clone();
                    let proto = Arc::clone(&proto);
                    conns.push(thread::spawn(move || {
                        let _ = proto.serve_conn(s, id, &svc, &sd);
                    }));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
            conns.retain(|h| !h.is_finished());
        }
        for h in conns {
            let _ = h.join();
        }
    });

    Ok(ServerHandle { addr, shutdown, accept: Some(accept), executor: Some(executor) })
}

/// Run a server forever (for the binaries).
pub fn run<P: Protocol>(cfg: ServerConfig, proto: Arc<P>, name: &str) -> io::Result<()> {
    let h = start(cfg, proto)?;
    eprintln!("[{name}] OneBarrier server listening on {}", h.addr);
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

/// Helper for protocol handlers: configure a freshly-accepted stream.
pub(crate) fn setup_stream(stream: &TcpStream) {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
}

/// Whether a read error is a benign timeout (so the loop can re-check shutdown).
pub(crate) fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

// ---------------------------------------------------------------------------
// RESP (Redis) protocol
// ---------------------------------------------------------------------------

use crate::resp::{read_command, Reply};

#[derive(Debug, Default)]
pub struct RespProtocol;

impl Protocol for RespProtocol {
    fn serve_conn(&self, stream: TcpStream, conn_id: u32, svc: &KvService, shutdown: &AtomicBool) -> io::Result<()> {
        setup_stream(&stream);
        let mut reader = io::BufReader::new(stream.try_clone()?);
        let mut writer = io::BufWriter::new(stream);
        let mut seq: u64 = 0;
        loop {
            match read_command(&mut reader) {
                Ok(None) => return Ok(()),
                Ok(Some(args)) if args.is_empty() => continue,
                Ok(Some(args)) => {
                    if args[0].eq_ignore_ascii_case(b"QUIT") {
                        Reply::ok().write_to(&mut writer)?;
                        writer.flush()?;
                        return Ok(());
                    }
                    seq += 1;
                    let reply = resp_dispatch(&args, conn_id, seq, svc);
                    reply.write_to(&mut writer)?;
                    writer.flush()?;
                }
                Err(ref e) if is_timeout(e) => {
                    if shutdown.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }
}

fn resp_dispatch(args: &[Vec<u8>], client: u32, seq: u64, svc: &KvService) -> Reply {
    let name = args[0].to_ascii_uppercase();
    let key = |i: usize| String::from_utf8_lossy(&args[i]).into_owned();
    match name.as_slice() {
        b"PING" => {
            if args.len() > 1 {
                Reply::Bulk(Some(args[1].clone()))
            } else {
                Reply::pong()
            }
        }
        b"SET" if args.len() >= 3 => match svc.apply(Op::set_bytes(client, seq, &key(1), &args[2])) {
            Some(_) => Reply::ok(),
            None => Reply::Error("ERR executor unavailable".into()),
        },
        b"GET" if args.len() >= 2 => match svc.apply(Op::get(client, seq, &key(1))) {
            Some(Output::Bytes(b)) => Reply::Bulk(b),
            _ => Reply::Bulk(None),
        },
        b"INCR" | b"DECR" | b"INCRBY" | b"DECRBY" if args.len() >= 2 => {
            let mag: i64 = if name == b"INCRBY" || name == b"DECRBY" {
                match args.get(2).and_then(|a| std::str::from_utf8(a).ok()?.parse().ok()) {
                    Some(v) => v,
                    None => return Reply::Error("ERR value is not an integer or out of range".into()),
                }
            } else {
                1
            };
            let delta = if name == b"DECR" || name == b"DECRBY" { -mag } else { mag };
            match svc.apply(Op::incr(client, seq, &key(1), delta)) {
                Some(Output::Value(Some(n))) => Reply::Int(n),
                _ => Reply::Error("ERR executor unavailable".into()),
            }
        }
        b"DEL" if args.len() >= 2 => {
            let mut count = 0i64;
            for (i, _) in args.iter().enumerate().skip(1) {
                if let Some(Output::Value(Some(n))) = svc.apply(Op::del(client, seq, &key(i))) {
                    count += n;
                }
            }
            Reply::Int(count)
        }
        b"DBSIZE" => Reply::Int(svc.len().unwrap_or(0) as i64),
        b"COMMAND" | b"CONFIG" => Reply::Array(vec![]),
        b"SELECT" | b"FLUSHALL" | b"FLUSHDB" | b"CLIENT" => Reply::ok(),
        b"RESET" => Reply::Simple("RESET".into()),
        b"INFO" => Reply::Bulk(Some(b"# Server\r\nredis_version:onebarrier-0.1\r\n".to_vec())),
        _ => Reply::Error(format!("ERR unknown command '{}'", String::from_utf8_lossy(&name))),
    }
}

/// Convenience: start a RESP (Redis-protocol) server.
pub fn start_resp(cfg: ServerConfig) -> io::Result<ServerHandle> {
    start(cfg, Arc::new(RespProtocol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::Ipv4Addr;

    fn tmpdir(tag: &str) -> PathBuf {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let mut d = std::env::temp_dir();
        d.push(format!("ob-kv-test-{}-{}-{}", tag, std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn cfg(dir: &PathBuf) -> ServerConfig {
        ServerConfig { addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0), dir: dir.clone(), snap_interval: 1000, fsync: false }
    }

    fn cmd(s: &mut TcpStream, parts: &[&[u8]]) -> Vec<u8> {
        let mut req = format!("*{}\r\n", parts.len()).into_bytes();
        for p in parts {
            req.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
            req.extend_from_slice(p);
            req.extend_from_slice(b"\r\n");
        }
        s.write_all(&req).unwrap();
        s.flush().unwrap();
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn resp_server_set_get_incr_and_recovers() {
        let dir = tmpdir("srv");
        {
            let h = start_resp(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            assert_eq!(cmd(&mut s, &[b"PING"]), b"+PONG\r\n");
            assert_eq!(cmd(&mut s, &[b"SET", b"foo", b"bar"]), b"+OK\r\n");
            assert_eq!(cmd(&mut s, &[b"GET", b"foo"]), b"$3\r\nbar\r\n");
            assert_eq!(cmd(&mut s, &[b"INCR", b"ctr"]), b":1\r\n");
            assert_eq!(cmd(&mut s, &[b"INCRBY", b"ctr", b"41"]), b":42\r\n");
            assert_eq!(cmd(&mut s, &[b"GET", b"missing"]), b"$-1\r\n");
            assert_eq!(cmd(&mut s, &[b"DEL", b"foo"]), b":1\r\n");
            drop(s);
            h.stop();
        }
        let h = start_resp(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        assert_eq!(cmd(&mut s, &[b"GET", b"foo"]), b"$-1\r\n", "DEL persisted");
        assert_eq!(cmd(&mut s, &[b"GET", b"ctr"]), b"$2\r\n42\r\n", "INCR state recovered");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
