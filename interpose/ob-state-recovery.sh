#!/usr/bin/env bash
# OneBarrier end-to-end STATE recovery of unmodified applications.
#   ob-state-recovery.sh <redis|node|all>
#
# The capstone: not just "an observable probe matches", but the FULL application
# state — including state DERIVED FROM time and RNG — is reconstructed byte-identical
# after a crash, by deterministic-replay of the request stream under the libOS
# (virtual clock + raw-getrandom determinizer). A CONTROL run without the libOS
# rebuilds DIFFERENT state, proving the libOS is what makes recovery exact.
#
#   redis : state = keys with TTLs (SET ... EX). Absolute expiry is server-time
#           derived; the recovered PTTLs must match byte-for-byte.
#   node  : state = a session store, each session {id: Math.random(), ts: Date.now()}
#           — derived from BOTH RNG and time. The recovered store must match.
#
# This is deterministic-replay recovery (OneBarrier's mechanism) applied to an
# UNMODIFIED binary, with the libOS pinning the residual local nondeterminism.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
NODE="${NODE:-node}"
IA='~0x4000000000000000:~0x0'
APP="${1:-all}"
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -lpthread

kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

# ---------- redis: TTL state ----------
redis_state() { # $1=port  -> emit a state fingerprint (key value pttl), sorted
  for k in $(redis-cli -p "$1" KEYS '*' | sort); do
    echo "$k=$(redis-cli -p "$1" GET "$k") pttl=$(redis-cli -p "$1" PTTL "$k")"
  done
}
redis_workload() { # $1=port — the request stream (the deterministic input)
  redis-cli -p "$1" SET session:1 alice EX 3600 >/dev/null
  redis-cli -p "$1" SET session:2 bob   EX 7200 >/dev/null
  redis-cli -p "$1" SET cache:x   42    PX 1500000 >/dev/null
  redis-cli -p "$1" SET cache:y   99    EX 600 >/dev/null
}
redis_run() { # $1=port $2=launch-prefix("" for control)  -> prints fingerprint
  kp "$1"
  eval "$2 redis-server --port $1 --save '' --appendonly no --logfile /tmp/sr$1.log &"
  for i in $(seq 40); do redis-cli -p "$1" PING 2>/dev/null|grep -q PONG && break; sleep 0.2; done
  redis_workload "$1"
  redis_state "$1"
  kp "$1"
}
redis_demo() {
  local VB=/tmp/sr-redis-vb; rm -f "$VB"
  local PRE="OB_VCLOCK=$VB LD_PRELOAD='$OBP'"
  echo "== redis: live (libOS) =="
  redis_run 6620 "$PRE" > /tmp/sr-redis-live.txt
  echo ">> crash; wait 3s real gap"; sleep 3
  echo "== redis: recovered (replay request stream, same vclock base) =="
  redis_run 6621 "$PRE" > /tmp/sr-redis-rec.txt
  echo "== redis: control (no libOS, real time) =="
  redis_run 6622 "" > /tmp/sr-redis-ctl.txt
  echo "--- state fingerprints ---"
  echo "live     : $(tr '\n' ' ' </tmp/sr-redis-live.txt)"
  echo "recovered: $(tr '\n' ' ' </tmp/sr-redis-rec.txt)"
  echo "control  : $(tr '\n' ' ' </tmp/sr-redis-ctl.txt)"
  if diff -q /tmp/sr-redis-live.txt /tmp/sr-redis-rec.txt >/dev/null && \
     ! diff -q /tmp/sr-redis-live.txt /tmp/sr-redis-ctl.txt >/dev/null; then
    echo "RESULT: redis STATE byte-identical across recovery (TTLs included), control differs ✅"
  else
    echo "RESULT: redis state mismatch ✗"; diff /tmp/sr-redis-live.txt /tmp/sr-redis-rec.txt
  fi
}

# ---------- node: session store derived from Math.random()+Date.now() ----------
node_demo() {
  cat > /tmp/sr-node.js <<'JS'
const http=require('http');const p=+process.argv[2];const store={};
http.createServer((q,s)=>{
  if(q.url==='/new'){ const id=Math.random().toString(36).slice(2);
    store[id]={id, ts:Date.now()}; s.end(id+'\n'); }
  else if(q.url==='/dump'){ const ks=Object.keys(store).sort();
    s.end(ks.map(k=>k+'@'+store[k].ts).join('\n')+'\n'); }
  else s.end('ok\n');
}).listen(p,'127.0.0.1');
JS
  local VB=/tmp/sr-node-vb VR=/tmp/sr-node-vr; rm -f "$VB" "$VR"
  local PRE="OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' setarch -R"
  up(){ for i in $(seq 50); do curl -s --max-time 1 localhost:$1/ >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
  workload(){ for i in $(seq 8); do curl -s --max-time 2 localhost:$1/new >/dev/null; done; }
  run(){ kp "$1"; eval "$2 '$NODE' /tmp/sr-node.js $1 >/dev/null 2>&1 &"
         up "$1" && workload "$1" && curl -s --max-time 2 localhost:$1/dump; kp "$1"; }
  echo "== node: live (libOS) =="
  run 8160 "$PRE" > /tmp/sr-node-live.txt
  echo ">> crash; wait 4s real gap"; sleep 4
  echo "== node: recovered (replay /new stream, same base+seed) =="
  run 8161 "$PRE" > /tmp/sr-node-rec.txt
  echo "== node: control (no libOS) =="
  run 8162 "" > /tmp/sr-node-ctl.txt
  echo "--- session store (id@timestamp) ---"
  echo "live     : $(tr '\n' ' ' </tmp/sr-node-live.txt)"
  echo "recovered: $(tr '\n' ' ' </tmp/sr-node-rec.txt)"
  echo "control  : $(tr '\n' ' ' </tmp/sr-node-ctl.txt)"
  if diff -q /tmp/sr-node-live.txt /tmp/sr-node-rec.txt >/dev/null && \
     ! diff -q /tmp/sr-node-live.txt /tmp/sr-node-ctl.txt >/dev/null; then
    echo "RESULT: node session STATE byte-identical across recovery (random IDs + timestamps), control differs ✅"
  else
    echo "RESULT: node state mismatch ✗"; diff /tmp/sr-node-live.txt /tmp/sr-node-rec.txt
  fi
}

case "$APP" in
  redis) redis_demo;;
  node)  node_demo;;
  all)   redis_demo; echo; node_demo;;
  *) echo "usage: $0 <redis|node|all>"; exit 2;;
esac
