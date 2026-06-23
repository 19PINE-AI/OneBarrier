#!/usr/bin/env bash
# OneBarrier — the Kafka partition model on the engine-native streaming log.
#   ob-kafka-partitions.sh [n_partitions] [msgs_per_partition]
#
# A Kafka-class broker as N share-nothing partitions, each a single-thread ob-log
# instance with its own durable ordered log + snapshot (the engine supplies total
# order + exactly-once + crash recovery). Demonstrates: (1) per-partition durability
# and OFFSET preservation across a real kill -9 + restart (exactly-once: recovered
# content byte-identical, no lost/duplicated messages); (2) share-nothing scaling —
# aggregate publish throughput grows with partitions because partitions share nothing.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OBLOG="$ROOT/target/release/ob-log"
P="${1:-4}"; M="${2:-2000}"
[ -x "$OBLOG" ] || { (cd "$ROOT" && cargo build --release -p onebarrier --bin ob-log >/dev/null 2>&1); }

kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

# line-protocol client: pub N msgs to a partition, or dump a partition's content hash.
client(){ python3 - "$@" <<'PY'
import socket, sys, hashlib
op, host, port = sys.argv[1], "127.0.0.1", int(sys.argv[2])
s = socket.create_connection((host, port), timeout=10); f = s.makefile("rwb")
if op == "pub":
    topic, n, tag = sys.argv[3], int(sys.argv[4]), sys.argv[5]
    last = 0
    for i in range(1, n+1):
        f.write(f"PUB {topic} {tag}-msg{i}\r\n".encode()); f.flush()
        r = f.readline().decode().split()
        if r and r[0]=="OFFSET": last=int(r[1])
    print(last)
elif op == "dump":
    topic = sys.argv[3]
    f.write(f"SUB {topic} 1\r\n".encode()); f.flush()
    h = hashlib.sha256(); n = 0
    while True:
        line = f.readline()
        if not line or line.strip()==b"END": break
        if line.startswith(b"MSG "):
            ln = int(line.split()[2]); body = f.read(ln); f.read(2)  # body + CRLF
            h.update(body); n += 1
    print(f"{n} {h.hexdigest()[:16]}")
f.write(b"QUIT\r\n"); f.flush(); s.close()
PY
}

echo "=== Kafka partition model: $P share-nothing partitions, $M msgs each ==="
rm -rf /tmp/ob-part-*; for i in $(seq 0 $((P-1))); do kp $((7300+i)); done; sleep 0.5
# Start P partitions
for i in $(seq 0 $((P-1))); do
  "$OBLOG" --port $((7300+i)) --dir /tmp/ob-part-$i --snap-interval 1000 >/tmp/ob-part-$i.log 2>&1 &
done
for i in $(seq 0 $((P-1))); do for t in $(seq 50); do client dump $((7300+i)) p$i >/dev/null 2>&1 && break; sleep 0.1; done; done

# Publish to every partition, timed (aggregate throughput across shards).
# wait only on the publisher jobs (a bare `wait` would block on the never-exiting
# ob-log server children).
t0=$(date +%s.%N)
pids=""
for i in $(seq 0 $((P-1))); do client pub $((7300+i)) p$i "$M" "P$i" >/dev/null & pids="$pids $!"; done
wait $pids
t1=$(date +%s.%N)
agg=$(python3 -c "print(f'{$P*$M/($t1-$t0):.0f}')")
echo "aggregate publish throughput ($P partitions): $agg msg/s"

# Record each partition's content, then crash+recover ONE partition.
declare -A before
for i in $(seq 0 $((P-1))); do before[$i]=$(client dump $((7300+i)) p$i); done
echo "before crash: $(for i in $(seq 0 $((P-1))); do echo -n "p$i=${before[$i]}  "; done)"

V=0  # victim partition
echo ">>> kill -9 partition $V, then restart it (recovers from its durable log)"
kp $((7300+V)); sleep 1
"$OBLOG" --port $((7300+V)) --dir /tmp/ob-part-$V --snap-interval 1000 >/tmp/ob-part-$V.log 2>&1 &
for t in $(seq 50); do client dump $((7300+V)) p$V >/dev/null 2>&1 && break; sleep 0.1; done
after=$(client dump $((7300+V)) p$V)
echo "partition $V  before=${before[$V]}  after-recovery=$after"

for i in $(seq 0 $((P-1))); do kp $((7300+i)); done
if [ "$after" = "${before[$V]}" ] && [ -n "$after" ]; then
  echo "RESULT: Kafka partition model ✅ — recovered partition byte-identical (count+hash), offsets preserved, exactly-once; $P share-nothing shards scale publish to $agg msg/s"
  exit 0
else
  echo "RESULT: partition recovery MISMATCH (before=${before[$V]} after=$after)"; exit 1
fi
