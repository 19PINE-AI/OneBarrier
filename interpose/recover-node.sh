#!/usr/bin/env bash
# Deterministic *time* recovery of an UNMODIFIED Node.js server via OneBarrier's
# time record/replay (the obpreload shim). The server returns Date.now() per
# request. We record a live run, crash it, wait so the wall clock advances, then
# replay: the recovered server reads the *recorded* time, so its Date.now() output
# is byte-identical to the live run — deterministic recovery. A control run (real
# time, no replay) shows the output WOULD differ without virtualization.
#
# Requires: node, curl, and interpose/libobpreload.so (gcc -shared -fPIC -O2 -o
# libobpreload.so obpreload.c -ldl -lpthread).
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SO="$HERE/libobpreload.so"
NODE="${NODE:-node}"
P=8095
SRV=/tmp/ob-tsserver.js
cat > "$SRV" <<'JS'
const http = require('http');
http.createServer((req,res)=>{ res.end(JSON.stringify({now:Date.now()})+'\n'); })
    .listen(8095,'127.0.0.1',()=>process.stderr.write('up\n'));
JS
drive(){ for i in $(seq 8); do curl -s --max-time 2 localhost:$P/; done; }
kill_srv(){ pkill -9 -f ob-tsserver.js 2>/dev/null; }
kill_srv; sleep 1; rm -f /tmp/ob-node-rec.bin /tmp/ob-node-{live,replay,control}.txt

echo "== 1. RECORD: drive unmodified node, record requests+time =="
OB_RECORD=/tmp/ob-node-rec.bin LD_PRELOAD="$SO" "$NODE" "$SRV" >/dev/null 2>&1 &
sleep 2.5; drive > /tmp/ob-node-live.txt; kill_srv; sleep 1

echo "== 2. CRASH, then wait 4s so the wall clock advances =="
sleep 4

echo "== 3. REPLAY: fresh node reads the recorded time =="
OB_REPLAY=/tmp/ob-node-rec.bin LD_PRELOAD="$SO" "$NODE" "$SRV" >/dev/null 2>&1 &
sleep 2.5; drive > /tmp/ob-node-replay.txt; kill_srv; sleep 1

echo "== 4. CONTROL: fresh node, real time (no replay) =="
LD_PRELOAD="$SO" "$NODE" "$SRV" >/dev/null 2>&1 &
sleep 2.5; drive > /tmp/ob-node-control.txt; kill_srv

echo
echo "live   Date.now: $(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-live.txt | head -3 | tr '\n' ' ')"
echo "replay Date.now: $(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-replay.txt | head -3 | tr '\n' ' ')"
echo "control Date.now: $(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-control.txt | head -3 | tr '\n' ' ')"
m=$(paste <(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-live.txt) <(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-replay.txt) | awk '$1==$2{c++} END{print c+0}')
n=$(grep -c now /tmp/ob-node-live.txt)
echo
echo "RESULT: $m/$n Date.now() values match live<->replay (deterministic time recovery)."
echo "        control (real time) differs from live: $([ "$(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-live.txt|head -1)" != "$(grep -oP '\"now\":\K[0-9]+' /tmp/ob-node-control.txt|head -1)" ] && echo YES || echo no)"
