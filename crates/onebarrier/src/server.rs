//! M3 (Track A) — a production-grade RESP key-value server on the OneBarrier
//! engine. A **single executor thread** owns the `Engine<KvStore>` and applies
//! every command in one serial order (the share-nothing / event-loop model);
//! connection threads parse RESP and hand commands to the executor over a
//! channel. Writes are durable (ordered log + timestamp snapshot) and the server
//! **recovers its state on restart** — transparent fault tolerance for a real
//! Redis-protocol service. Speaks to `redis-cli` and `redis-benchmark`.

use std::io::{self, BufReader, BufWriter};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use onepipe_core::timestamp::Timestamp;

use crate::resp::{read_command, Reply};
use crate::{Engine, KvStore, Op, Output};

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub addr: SocketAddr,
    pub dir: PathBuf,
    pub snap_interval: u64,
    pub fsync: bool,
}

/// One unit of work sent to the executor.
struct Job {
    args: Vec<Vec<u8>>,
    client: u32,
    seq: u64,
    reply: SyncSender<Reply>,
}

/// A running server. `stop()` shuts the accept loop, drains connections, and
/// joins the executor so its durable store is fully closed (safe to recover).
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

/// Start the server in the background; returns once it is bound and accepting.
pub fn start(cfg: ServerConfig) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(cfg.addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let shutdown = Arc::new(AtomicBool::new(false));

    // Executor: owns the engine, applies commands in one serial order.
    let (tx, rx) = mpsc::channel::<Job>();
    let exec_dir = cfg.dir.clone();
    let executor = thread::spawn(move || -> io::Result<()> {
        let mut engine = if exec_dir.join("snapshot").exists() || exec_dir.join("oplog").exists() {
            Engine::<KvStore>::recover(&exec_dir, cfg.snap_interval, cfg.fsync)?
        } else {
            Engine::<KvStore>::create(&exec_dir, cfg.snap_interval, cfg.fsync)?
        };
        let mut clock: u64 = engine.last_ts() + 1;
        while let Ok(job) = rx.recv() {
            let reply = exec_command(&mut engine, &job, &mut clock)?;
            let _ = job.reply.send(reply);
        }
        Ok(())
    });

    let conn_id = Arc::new(AtomicU32::new(1));
    let accept_shutdown = Arc::clone(&shutdown);
    let accept = thread::spawn(move || {
        // `tx` lives here; cloned into each connection. When the accept loop
        // ends and all connections finish, every sender drops and the executor
        // returns from `rx.recv()`.
        let tx = tx;
        let mut conns: Vec<JoinHandle<()>> = Vec::new();
        for stream in listener.incoming() {
            if accept_shutdown.load(Ordering::SeqCst) {
                break;
            }
            match stream {
                Ok(s) => {
                    let tx = tx.clone();
                    let id = conn_id.fetch_add(1, Ordering::Relaxed);
                    let sd = Arc::clone(&accept_shutdown);
                    conns.push(thread::spawn(move || {
                        let _ = handle_conn(s, id, tx, &sd);
                    }));
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(_) => break,
            }
            conns.retain(|h| !h.is_finished());
        }
        drop(tx);
        for h in conns {
            let _ = h.join();
        }
    });

    Ok(ServerHandle { addr, shutdown, accept: Some(accept), executor: Some(executor) })
}

/// Run forever (for the `ob-kv` binary).
pub fn run(cfg: ServerConfig) -> io::Result<()> {
    let h = start(cfg)?;
    eprintln!("[ob-kv] OneBarrier RESP KV server listening on {}", h.addr);
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn handle_conn(stream: TcpStream, id: u32, tx: Sender<Job>, shutdown: &AtomicBool) -> io::Result<()> {
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);
    let mut seq: u64 = 0;
    loop {
        match read_command(&mut reader) {
            Ok(None) => return Ok(()), // clean EOF
            Ok(Some(args)) if args.is_empty() => continue,
            Ok(Some(args)) => {
                let upper = args[0].to_ascii_uppercase();
                if upper == b"QUIT" {
                    Reply::ok().write_to(&mut writer)?;
                    use std::io::Write;
                    writer.flush()?;
                    return Ok(());
                }
                seq += 1;
                let (rtx, rrx) = mpsc::sync_channel(1);
                if tx.send(Job { args, client: id, seq, reply: rtx }).is_err() {
                    return Ok(()); // executor gone
                }
                match rrx.recv() {
                    Ok(reply) => {
                        reply.write_to(&mut writer)?;
                        use std::io::Write;
                        writer.flush()?;
                    }
                    Err(_) => return Ok(()),
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                if shutdown.load(Ordering::SeqCst) {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// Per-server logical clock for engine timestamps (monotonic, unique).
static GLOBAL_TICK: AtomicU64 = AtomicU64::new(1);

fn exec_command(engine: &mut Engine<KvStore>, job: &Job, clock: &mut u64) -> io::Result<Reply> {
    let args = &job.args;
    let name = args[0].to_ascii_uppercase();
    let mut next_ts = || {
        *clock += 1;
        // Keep timestamps globally unique-ish and monotonic.
        let g = GLOBAL_TICK.fetch_add(1, Ordering::Relaxed);
        Timestamp::from_nanos((*clock).max(g))
    };

    let reply = match name.as_slice() {
        b"PING" => {
            if args.len() > 1 {
                Reply::Bulk(Some(args[1].clone()))
            } else {
                Reply::pong()
            }
        }
        b"SET" if args.len() >= 3 => {
            let key = String::from_utf8_lossy(&args[1]).into_owned();
            let op = Op::set_bytes(job.client, job.seq, &key, &args[2]);
            engine.deliver(next_ts(), &op)?;
            Reply::ok()
        }
        b"GET" if args.len() >= 2 => {
            let key = String::from_utf8_lossy(&args[1]).into_owned();
            let op = Op::get(job.client, job.seq, &key);
            match engine.deliver(next_ts(), &op)? {
                Output::Bytes(b) => Reply::Bulk(b),
                _ => Reply::Bulk(engine.state().get_bytes(&key).map(<[u8]>::to_vec)),
            }
        }
        b"INCR" | b"DECR" | b"INCRBY" | b"DECRBY" if args.len() >= 2 => {
            let key = String::from_utf8_lossy(&args[1]).into_owned();
            let mag: i64 = if name == b"INCRBY" || name == b"DECRBY" {
                match args.get(2).and_then(|a| std::str::from_utf8(a).ok()?.parse().ok()) {
                    Some(v) => v,
                    None => return Ok(Reply::Error("ERR value is not an integer or out of range".into())),
                }
            } else {
                1
            };
            let delta = if name == b"DECR" || name == b"DECRBY" { -mag } else { mag };
            let op = Op::incr(job.client, job.seq, &key, delta);
            match engine.deliver(next_ts(), &op)? {
                Output::Value(Some(n)) => Reply::Int(n),
                _ => Reply::Int(engine.state().get_int(&key).unwrap_or(0)),
            }
        }
        b"DEL" if args.len() >= 2 => {
            let mut count = 0i64;
            for k in &args[1..] {
                let key = String::from_utf8_lossy(k).into_owned();
                let op = Op::del(job.client, job.seq, &key);
                if let Output::Value(Some(n)) = engine.deliver(next_ts(), &op)? {
                    count += n;
                }
            }
            Reply::Int(count)
        }
        b"DBSIZE" => Reply::Int(engine.state().len() as i64),
        b"EXISTS" if args.len() >= 2 => {
            let key = String::from_utf8_lossy(&args[1]).into_owned();
            Reply::Int(i64::from(engine.state().get_bytes(&key).is_some()))
        }
        // Handshake / probe commands used by redis-cli & redis-benchmark.
        b"COMMAND" => Reply::Array(vec![]),
        b"CONFIG" => Reply::Array(vec![]),
        b"HELLO" => Reply::Error("ERR unsupported (RESP2 only)".into()),
        b"SELECT" | b"FLUSHALL" | b"FLUSHDB" | b"CLIENT" => Reply::ok(),
        b"RESET" => Reply::Simple("RESET".into()),
        b"INFO" => Reply::Bulk(Some(b"# Server\r\nredis_version:onebarrier-0.1\r\n".to_vec())),
        _ => Reply::Error(format!(
            "ERR unknown or malformed command '{}'",
            String::from_utf8_lossy(&name)
        )),
    };
    Ok(reply)
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
        ServerConfig {
            addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0),
            dir: dir.clone(),
            snap_interval: 1000,
            fsync: false,
        }
    }

    /// Send a RESP array command and read one reply line/bulk.
    fn cmd(s: &mut TcpStream, parts: &[&[u8]]) -> Vec<u8> {
        let mut req = format!("*{}\r\n", parts.len()).into_bytes();
        for p in parts {
            req.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
            req.extend_from_slice(p);
            req.extend_from_slice(b"\r\n");
        }
        s.write_all(&req).unwrap();
        s.flush().unwrap();
        // Read a chunk (replies here are small and one-shot).
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn resp_server_set_get_incr_and_recovers() {
        let dir = tmpdir("srv");
        // --- run 1: write some data ---
        let addr = {
            let h = start(cfg(&dir)).unwrap();
            let mut s = TcpStream::connect(h.addr).unwrap();
            assert_eq!(cmd(&mut s, &[b"PING"]), b"+PONG\r\n");
            assert_eq!(cmd(&mut s, &[b"SET", b"foo", b"bar"]), b"+OK\r\n");
            assert_eq!(cmd(&mut s, &[b"GET", b"foo"]), b"$3\r\nbar\r\n");
            assert_eq!(cmd(&mut s, &[b"INCR", b"ctr"]), b":1\r\n");
            assert_eq!(cmd(&mut s, &[b"INCRBY", b"ctr", b"41"]), b":42\r\n");
            assert_eq!(cmd(&mut s, &[b"GET", b"missing"]), b"$-1\r\n");
            assert_eq!(cmd(&mut s, &[b"DEL", b"foo"]), b":1\r\n");
            let a = h.addr;
            drop(s);
            h.stop(); // joins executor → durable store closed
            a
        };
        let _ = addr;

        // --- run 2: recover and verify persistence ---
        let h = start(cfg(&dir)).unwrap();
        let mut s = TcpStream::connect(h.addr).unwrap();
        assert_eq!(cmd(&mut s, &[b"GET", b"foo"]), b"$-1\r\n", "DEL persisted");
        assert_eq!(cmd(&mut s, &[b"GET", b"ctr"]), b"$2\r\n42\r\n", "INCR state recovered");
        drop(s);
        h.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
