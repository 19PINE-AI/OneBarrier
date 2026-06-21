#!/usr/bin/env bash
# Transparent record-replay fault tolerance for an UNMODIFIED redis-server,
# via the OneBarrier obpreload LD_PRELOAD shim + ob-replay. Redis has no
# knowledge of OneBarrier: the shim transparently intercepts its socket I/O,
# captures the request stream, and ob-replay rebuilds state on a fresh instance
# after a crash.
#
# Requires: gcc, redis-server, redis-cli, and the ob-replay binary
#   (cargo build --release -p onebarrier --bin ob-replay).
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
PORT=6390
CAP=/tmp/ob-capture.log
SO="$HERE/libobpreload.so"
REPLAY="$ROOT/target/release/ob-replay"

cleanup() { pkill -9 -f "redis-server.*$PORT" 2>/dev/null; }
trap cleanup EXIT
cleanup; sleep 1; rm -f "$CAP"

echo "== build shim =="
gcc -shared -fPIC -O2 -o "$SO" "$HERE/obpreload.c" -ldl -lpthread || exit 1

echo "== 1. start UNMODIFIED redis-server under the obpreload shim =="
OB_CAPTURE="$CAP" LD_PRELOAD="$SO" nohup redis-server --port $PORT --save '' --appendonly no >/tmp/r-preload.log 2>&1 &
sleep 2

echo "== 2. populate state with real redis-cli =="
for i in 1 2 3 4 5; do redis-cli -p $PORT SET key$i value$i >/dev/null; done
redis-cli -p $PORT SET name OneBarrier >/dev/null
redis-cli -p $PORT INCR hits >/dev/null; redis-cli -p $PORT INCR hits >/dev/null
echo "   before crash: DBSIZE=$(redis-cli -p $PORT DBSIZE) name=$(redis-cli -p $PORT GET name) hits=$(redis-cli -p $PORT GET hits)"
echo "   intercepted:  $(wc -c <"$CAP") bytes captured transparently"

echo "== 3. CRASH (kill -9) =="
pkill -9 -f "redis-server.*$PORT"; sleep 1

echo "== 4. fresh, empty redis-server (no persistence) =="
nohup redis-server --port $PORT --save '' --appendonly no >/tmp/r-fresh.log 2>&1 &
sleep 2
echo "   fresh:        DBSIZE=$(redis-cli -p $PORT DBSIZE) name=$(redis-cli -p $PORT GET name)  <- state lost"

echo "== 5. TRANSPARENT RECOVERY: replay the intercepted request stream =="
"$REPLAY" --capture "$CAP" --target 127.0.0.1:$PORT
echo "   after replay: DBSIZE=$(redis-cli -p $PORT DBSIZE) name=$(redis-cli -p $PORT GET name) hits=$(redis-cli -p $PORT GET hits)"
echo "   keys: $(redis-cli -p $PORT KEYS '*' | sort | tr '\n' ' ')"
