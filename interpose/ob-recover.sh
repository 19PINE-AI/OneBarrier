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

if [ "$APP" = all ]; then
  rc=0
  for a in redis memcached nginx node; do
    echo "############## $a ##############"
    bash "$0" "$a" "$GAP" | grep -E '^(===|live|replay|control|RESULT)'
    [ "${PIPESTATUS[0]}" = 0 ] || rc=1
    echo
  done
  exit $rc
fi
VB="/tmp/ob-vbase-$APP"; VR="/tmp/ob-vrand-$APP"
L="/tmp/ob-$APP-live.txt"; R="/tmp/ob-$APP-replay.txt"; C="/tmp/ob-$APP-control.txt"
RNG="$HERE/librngdet.so"
IA='~0x4000000000000000:~0x0'         # disable RDRAND/RDSEED for OpenSSL (CPU instr, untrappable)
# Full determinism stack: virtual clock (time) + seccomp getrandom trap (raw RNG)
# + ASLR-off (setarch -R) + no-RDRAND. The recovered process re-derives identical
# time- AND randomness-dependent output.
PRE="OPENSSL_ia32cap='$IA' OB_VCLOCK='$VB' OB_VRAND='$VR' LD_PRELOAD='$RNG $SO' setarch -R"
CTL="LD_PRELOAD='$SO'"                 # control: real time + real randomness

case "$APP" in
  redis)
    P1=6520; P2=6521; P3=6522
    bcmd(){ echo "redis-server --port $1 --save '' --appendonly no --logfile /tmp/ob-r$1.log"; }
    lcmd(){ echo "$PRE $(bcmd "$1")"; }
    up(){ for i in $(seq 50); do redis-cli -p "$1" PING 2>/dev/null | grep -q PONG && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do redis-cli -p "$1" TIME 2>/dev/null | tr '\n' '.'; echo; done; }
    ;;
  memcached)
    P1=11230; P2=11231; P3=11232
    bcmd(){ echo "memcached -p $1 -U 0 -t 1 -m 64"; }
    lcmd(){ echo "$PRE $(bcmd "$1")"; }
    up(){ for i in $(seq 50); do printf 'version\r\n' | timeout 1 nc 127.0.0.1 "$1" 2>/dev/null | grep -q VERSION && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do printf 'stats\r\nquit\r\n' | timeout 2 nc 127.0.0.1 "$1" 2>/dev/null | grep '^STAT time ' | tr -d '\r'; done; }
    ;;
  nginx)
    P1=8130; P2=8131; P3=8132
    mkcfg(){ cat > /tmp/ob-ngx-$1.conf <<NG
daemon off; master_process off; worker_processes 1;
error_log /tmp/ob-ngx-$1.err; pid /tmp/ob-ngx-$1.pid;
events { worker_connections 64; }
http { access_log off; server { listen $1; location / { return 200 'ok\n'; } } }
NG
}
    bcmd(){ mkcfg "$1"; echo "nginx -c /tmp/ob-ngx-$1.conf -p /tmp"; }
    lcmd(){ echo "$PRE $(bcmd "$1")"; }
    up(){ for i in $(seq 50); do curl -s --max-time 1 "localhost:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do curl -s -D - --max-time 2 "localhost:$1/" 2>/dev/null | grep -i '^Date:' | tr -d '\r'; done; }
    ;;
  node)
    P1=8140; P2=8141; P3=8142
    cat > /tmp/ob-node-srv.js <<'JS'
const http=require('http');const p=+process.argv[2];
http.createServer((q,s)=>s.end(JSON.stringify({now:Date.now(),rnd:Math.random()})+'\n')).listen(p,'127.0.0.1');
JS
    bcmd(){ echo "'$NODE' /tmp/ob-node-srv.js $1"; }
    lcmd(){ echo "$PRE $(bcmd "$1")"; }
    up(){ for i in $(seq 50); do curl -s --max-time 1 "localhost:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
    drive(){ for i in $(seq 6); do curl -s --max-time 2 "localhost:$1/" 2>/dev/null; done; }
    ;;
  *) echo "unknown app: $APP"; exit 2;;
esac

kill_port(){ for pid in $(ss -tlnp 2>/dev/null | grep ":$1 " | grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -lpthread 2>/dev/null
kill_port "$P1"; kill_port "$P2"; kill_port "$P3"; sleep 1; rm -f "$VB" "$VR" "$L" "$R" "$C"

# 1) LIVE — record under the virtual clock
eval "$(lcmd "$P1") >/dev/null 2>&1 &"
up "$P1" && drive "$P1" > "$L"
kill_port "$P1"; sleep 1
echo ">>> crash; wait ${GAP}s so the real wall clock advances"
sleep "$GAP"
# 2) REPLAY — fresh instance, SAME persisted base ⇒ deterministic time
eval "$(lcmd "$P2") >/dev/null 2>&1 &"
up "$P2" && drive "$P2" > "$R"
kill_port "$P2"; sleep 1
# 3) CONTROL — fresh instance, NO virtual clock (real time) ⇒ must differ
eval "$CTL $(bcmd "$P3") >/dev/null 2>&1 &"
up "$P3" && drive "$P3" > "$C"
kill_port "$P3"

echo "=== $APP — deterministic recovery under the virtual clock (${GAP}s real gap) ==="
echo "live   : $(head -2 "$L" 2>/dev/null | tr '\n' ' ')"
echo "replay : $(head -2 "$R" 2>/dev/null | tr '\n' ' ')"
echo "control: $(head -2 "$C" 2>/dev/null | tr '\n' ' ')   (no virtual clock — real time)"
ident=no; diff -q "$L" "$R" >/dev/null 2>&1 && [ -s "$L" ] && ident=yes
ctldiff=no; [ -s "$C" ] && ! diff -q "$L" "$C" >/dev/null 2>&1 && ctldiff=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ]; then
  echo "RESULT: $APP DETERMINISTIC ✅ — replay byte-identical to live, control (real time) differs"; exit 0
elif [ "$ident" = yes ]; then
  echo "RESULT: $APP replay byte-identical, but control did not differ (weak — check gap)"; exit 0
else
  echo "RESULT: $APP NOT byte-identical (live_empty=$([ -s "$L" ]&&echo no||echo yes) replay_empty=$([ -s "$R" ]&&echo no||echo yes))"; exit 1
fi
