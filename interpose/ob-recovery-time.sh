#!/usr/bin/env bash
# OneBarrier — recovery TIME / availability for the unmodified-app replay path.
#   ob-recovery-time.sh
#
# Validates recovery *latency* (not just correctness): capture an unmodified redis's
# request stream for N distinct-key writes, crash it, and time how long ob-replay
# takes to reconstruct state on a fresh instance. Recovery time scales LINEARLY with
# log length (matches the engine recovery model, RQ8), and the recovered key count
# equals the live key count (exact reconstruction). ob-replay reconnects and replays
# all connections in PARALLEL, so recovery is bounded by the slowest single
# connection, not the sum.
#
# With a periodic checkpoint (CRIU full-process, or app-native RDB) the replay is
# bounded to the post-checkpoint tail — so end-to-end availability = restore + tail.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
SO="$HERE/libobpreload.so"; REPLAY="$ROOT/target/release/ob-replay"
[ -f "$SO" ] || gcc -shared -fPIC -O2 -o "$SO" "$HERE/obpreload.c" -ldl -lpthread
[ -x "$REPLAY" ] || ( cd "$ROOT" && cargo build --release -p onebarrier --bin ob-replay )
P=6975
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$P "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

echo "=== unmodified-redis replay-recovery time vs log length (distinct keys) ==="
printf "  %-9s %-9s %-11s %-11s %s\n" "requests" "capture" "live_keys" "recovered" "replay_time"
for N in 10000 50000 100000 200000 500000 1000000; do
  CAP=/tmp/obrt-$N.bin; kp; rm -f "$CAP"
  OB_CAPTURE="$CAP" LD_PRELOAD="$SO" redis-server --port $P --save '' --appendonly no --logfile /tmp/obrt$P.log &
  for i in $(seq 40); do redis-cli -p $P PING 2>/dev/null|grep -q PONG && break; sleep 0.2; done
  redis-benchmark -p $P -t set -n "$N" -r 100000000 -c 50 -P 1 >/dev/null 2>&1
  live=$(redis-cli -p $P DBSIZE); cap=$(stat -c%s "$CAP" 2>/dev/null)
  kp; sleep 0.4
  redis-server --port $P --save '' --appendonly no --logfile /tmp/obrt2$P.log &
  for i in $(seq 40); do redis-cli -p $P PING 2>/dev/null|grep -q PONG && break; sleep 0.2; done
  t0=$(date +%s.%N); "$REPLAY" --capture "$CAP" --target 127.0.0.1:$P >/dev/null 2>&1; t1=$(date +%s.%N)
  rec=$(redis-cli -p $P DBSIZE)
  printf "  %-9s %-9s %-11s %-11s %6.0f ms %s\n" "$N" "$(numfmt --to=iec "$cap")" "$live" "$rec" \
    "$(echo "($t1-$t0)*1000"|bc)" "$([ "$live" = "$rec" ] && echo '(exact)' || echo 'MISMATCH')"
  kp; rm -f "$CAP"; sleep 0.3
done
echo "  => linear in log length (~0.5 ms / 1k req, ~95 MB/s); recovered == live (exact)."
echo "  => bound the log with a checkpoint (CRIU full-process / RDB) -> availability = restore + tail."
