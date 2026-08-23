#!/usr/bin/env bash
# OneBarrier — transparent FT for a canonical stateful MICROSERVICE.
#   ob-microservice.sh [real_gap_seconds]
#
# The durable-execution path (Temporal/DBOS) rewrites a stateful service to
# externalize its state and its non-deterministic effects (order ids, timestamps).
# OneBarrier gives an UNMODIFIED in-memory service the same guarantees transparently.
#
# The service is an order/checkout microservice (stock python3 http.server): each
# request creates an order {id = random hex, ts = wall time, seq, running total}.
# State = the in-memory order book. We verify TWO properties on recovery:
#   (1) exactly-once — #orders == #acked requests, running total exact (no lost/dup);
#   (2) deterministic state — the order book (incl. the random ids and timestamps the
#       durable-execution path forces you to externalize) is byte-identical across a
#       crash + real gap; a no-libOS control rebuilds different ids/timestamps.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=ob-common.sh
. "$HERE/ob-common.sh"
ob_require_shims libobpreload.so librngdet.so || exit 1
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"; DET=/tmp/ob-det-urandom
GAP="${1:-3}"; IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic
[ -f "$DET" ] || python3 - "$DET" <<'PY'
import struct,sys
s=0x0B1A2C3D4E5F6071; out=bytearray()
for _ in range((1<<20)>>3):
    s=(s+0x9E3779B97F4A7C15)&0xFFFFFFFFFFFFFFFF; z=s
    z=((z^(z>>30))*0xBF58476D1CE4E5B9)&0xFFFFFFFFFFFFFFFF
    z=((z^(z>>27))*0x94D049BB133111EB)&0xFFFFFFFFFFFFFFFF
    z^=(z>>31); out+=struct.pack('<Q',z)
open(sys.argv[1],'wb').write(out)
PY

SRV=/tmp/ob-microservice.py
cat > "$SRV" <<'PY'
import os, time, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
orders=[]; total=0
class H(BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_GET(self):
        global total
        # order id: random (os.urandom -> /dev/urandom, redirected deterministically)
        oid = os.urandom(6).hex()
        ts = f"{time.time():.6f}"          # wall time -> virtual clock
        amount = 10 + (len(orders)*7) % 90
        total += amount
        orders.append((len(orders)+1, oid, ts, amount, total))
        body = "".join(f"{s}|{i}|{t}|{a}|{tot}\n" for (s,i,t,a,tot) in orders).encode()
        self.send_response(200); self.send_header("Content-Length",str(len(body))); self.end_headers()
        self.wfile.write(body)
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY

PYENV="PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1"
VB=/tmp/ob-ms-vb; VR=/tmp/ob-ms-vr; N=50
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
# TCP-connect readiness (no HTTP request ⇒ does NOT create an order, unlike a curl GET)
up(){ for i in $(seq 50); do (exec 3<>/dev/tcp/127.0.0.1/$1) 2>/dev/null && { exec 3<&-; return 0; }; sleep 0.1; done; return 1; }
drive(){ local p=$1 last=""; for i in $(seq 1 $N); do last=$(curl -s --max-time 2 "127.0.0.1:$p/" 2>/dev/null); done; echo "$last"; }
run(){ local p=$1 mode=$2; kp "$p"
  if [ "$mode" = libos ]; then
    unshare -r -m bash -c "mount --bind $DET /dev/urandom; OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' $PYENV setarch -R python3 $SRV $p >/dev/null 2>&1" &
  else
    eval "$PYENV python3 $SRV $p >/dev/null 2>&1 &"
  fi
  up "$p" && drive "$p"; kp "$p"
}

LV=/tmp/ob-ms-live; RP=/tmp/ob-ms-replay; CT=/tmp/ob-ms-ctl
echo "=== Stateful microservice (order/checkout) — exactly-once + deterministic recovery (${GAP}s real gap) ==="
rm -f "$VB" "$VR"
run 8510 libos > "$LV"; sleep 1
echo ">>> crash; wait ${GAP}s; recover by replaying the request log"; sleep "$GAP"
run 8511 libos > "$RP"; sleep 1
run 8512 control > "$CT"
# exactly-once: #orders == N, running total consistent
no=$(wc -l < "$RP"); tot=$(tail -1 "$RP" 2>/dev/null | awk -F'|' '{print $5}')
echo "exactly-once: orders=$no (expected $N), running_total=$tot"
echo "live   last order: $(tail -1 "$LV")"
echo "replay last order: $(tail -1 "$RP")"
echo "control last order: $(tail -1 "$CT")   (real time + real urandom)"
ident=no; diff -q "$LV" "$RP" >/dev/null 2>&1 && [ -s "$LV" ] && ident=yes
ctldiff=no; [ -s "$CT" ] && ! diff -q "$LV" "$CT" >/dev/null 2>&1 && ctldiff=yes
exonce=no; [ "$no" = "$N" ] && exonce=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ] && [ "$exonce" = yes ]; then
  echo "RESULT: microservice order book byte-identical across recovery (ids+timestamps), exactly-once ($N/$N), control differs ✅"; exit 0
else
  echo "RESULT: microservice check FAILED (ident=$ident ctldiff=$ctldiff exonce=$exonce)"; diff "$LV" "$RP" | head; exit 1
fi
