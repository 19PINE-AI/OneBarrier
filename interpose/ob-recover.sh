#!/usr/bin/env bash
# OneBarrier deterministic-recovery harness (production).
#   ob-recover.sh <redis|memcached|nginx|node> [real_gap_seconds]
# Records an UNMODIFIED app under the virtual clock (OB_VCLOCK), drives a
# time-dependent workload, crashes it, waits a real-time gap, replays on a fresh
# instance (same persisted base), and verifies time-dependent output is
# BYTE-IDENTICAL across the gap — deterministic recovery.
#
# NOTE: the server is launched as a literal command string via `eval "$cmd &"`
# (NOT a backgrounded shell function) — backgrounding a function is dropped by
# some sandboxes, whereas eval-backgrounding a literal command is robust.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SO="$HERE/libobpreload.so"
APP="${1:-redis}"; GAP="${2:-3}"
NODE="$(command -v node || echo /home/ubuntu/.local/bin/node)"
VB="/tmp/ob-vbase-$APP"; L="/tmp/ob-$APP-live.txt"; R="/tmp/ob-$APP-replay.txt"
PRE="OB_VCLOCK=$VB LD_PRELOAD=$SO"

case "$APP" in
  redis)
    P1=6520; P2=6521
    lcmd(){ echo "$PRE redis-server --port $1 --save '' --appendonly no --logfile /tmp/ob-r$1.log"; }
    up(){ for i in $(seq 50); do redis-cli -p "$1" PING 2>/dev/null | grep -q PONG && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do redis-cli -p "$1" TIME 2>/dev/null | tr '\n' '.'; echo; done; }
    ;;
  memcached)
    P1=11230; P2=11231
    lcmd(){ echo "$PRE memcached -p $1 -U 0 -t 1 -m 64"; }
    up(){ for i in $(seq 50); do printf 'version\r\n' | timeout 1 nc 127.0.0.1 "$1" 2>/dev/null | grep -q VERSION && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do printf 'stats\r\nquit\r\n' | timeout 2 nc 127.0.0.1 "$1" 2>/dev/null | grep '^STAT time ' | tr -d '\r'; done; }
    ;;
  nginx)
    P1=8130; P2=8131
    mkcfg(){ cat > /tmp/ob-ngx-$1.conf <<NG
daemon off; master_process off; worker_processes 1;
error_log /tmp/ob-ngx-$1.err; pid /tmp/ob-ngx-$1.pid;
events { worker_connections 64; }
http { access_log off; server { listen $1; location / { return 200 'ok\n'; } } }
NG
}
    lcmd(){ mkcfg "$1"; echo "$PRE nginx -c /tmp/ob-ngx-$1.conf -p /tmp"; }
    up(){ for i in $(seq 50); do curl -s --max-time 1 "localhost:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do curl -s -D - --max-time 2 "localhost:$1/" 2>/dev/null | grep -i '^Date:' | tr -d '\r'; done; }
    ;;
  node)
    P1=8140; P2=8141
    cat > /tmp/ob-node-srv.js <<'JS'
const http=require('http');const p=+process.argv[2];
http.createServer((q,s)=>s.end(JSON.stringify({now:Date.now()})+'\n')).listen(p,'127.0.0.1');
JS
    lcmd(){ echo "$PRE '$NODE' /tmp/ob-node-srv.js $1"; }
    up(){ for i in $(seq 50); do curl -s --max-time 1 "localhost:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do curl -s --max-time 2 "localhost:$1/" 2>/dev/null; done; }
    ;;
  *) echo "unknown app: $APP"; exit 2;;
esac

kill_port(){ for pid in $(ss -tlnp 2>/dev/null | grep ":$1 " | grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
kill_port "$P1"; kill_port "$P2"; sleep 1; rm -f "$VB" "$L" "$R"

eval "$(lcmd "$P1") >/dev/null 2>&1 &"
up "$P1" && drive "$P1" > "$L"
kill_port "$P1"; sleep 1
echo ">>> crash; wait ${GAP}s so the real wall clock advances"
sleep "$GAP"
eval "$(lcmd "$P2") >/dev/null 2>&1 &"
up "$P2" && drive "$P2" > "$R"
kill_port "$P2"

echo "=== $APP — deterministic recovery under the virtual clock (${GAP}s real gap) ==="
echo "live  : $(head -2 "$L" 2>/dev/null | tr '\n' ' ')"
echo "replay: $(head -2 "$R" 2>/dev/null | tr '\n' ' ')"
if [ -s "$L" ] && [ -s "$R" ] && diff -q "$L" "$R" >/dev/null 2>&1; then
  echo "RESULT: $APP DETERMINISTIC — time-dependent output byte-identical across recovery"; exit 0
else
  echo "RESULT: $APP NOT byte-identical (live_empty=$([ -s "$L" ]&&echo no||echo yes) replay_empty=$([ -s "$R" ]&&echo no||echo yes))"; exit 1
fi
