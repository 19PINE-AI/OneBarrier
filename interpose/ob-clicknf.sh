#!/usr/bin/env bash
# OneBarrier — transparent fault tolerance for a SOFTWARE network function.
#   ob-clicknf.sh [real_gap_seconds]
#
# A software Click NF (OpenClickNP FlowCache + L4LoadBalancer, SIGCOMM'16 lineage)
# run on the host CPU under the OneBarrier libOS. It is a stateful L4 load balancer
# with connection tracking: each flow keeps a backend assignment (affinity) and a
# conntrack last-seen timestamp. Packets arrive over a socket, so the virtual clock
# advances per packet and the last-seen timestamps are a deterministic function of
# the packet sequence.
#
# Result: replaying the (fabric-ordered) packet log after a kill -9 rebuilds the
# EXACT flow table — backend affinity AND conntrack timers — byte-identical to the
# pre-crash table. A no-libOS control (real time) rebuilds different timestamps. This
# is the FTMB / Pico-Replication stateful-middlebox-FT problem, solved transparently.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
NF="$HERE/clicknf/ob_clicknf"
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
GAP="${1:-3}"; IA='~0x4000000000000000:~0x0'
[ -x "$NF" ] || g++ -std=c++17 -O2 -I /home/ubuntu/OpenClickNP/runtime/include "$HERE/clicknf/ob_clicknf.cpp" -o "$NF"
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic

VB=/tmp/ob-clicknf-vb; VR=/tmp/ob-clicknf-vr
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
up(){ for i in $(seq 50); do (exec 3<>/dev/tcp/127.0.0.1/$1) 2>/dev/null && { exec 3<&-; return 0; }; sleep 0.1; done; return 1; }

# Synchronous packet driver: a deterministic flow trace (200 packets over 60 flows,
# with reuse → MISS on first sight, HIT on return). One packet per request/reply so
# the read() chunking — hence the per-packet virtual-clock ticks — is deterministic.
drive(){ python3 - "$1" <<'PY'
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=10); f = s.makefile("rwb")
for i in range(200):
    flow = (i*7 + 13) % 60          # deterministic trace with flow reuse
    f.write(f"P {flow}\n".encode()); f.flush(); f.readline()
f.write(b"DUMP\n"); f.flush()
out = []
while True:
    line = f.readline()
    if not line or line.strip()==b"END": break
    out.append(line.decode())
sys.stdout.write("".join(out))
f.write(b"QUIT\n"); f.flush(); s.close()
PY
}

# $1=port $2=mode(libos|control)
run(){ local p=$1 mode=$2; kp "$p"
  if [ "$mode" = libos ]; then
    eval "OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' setarch -R '$NF' $p >/dev/null 2>&1 &"
  else
    eval "'$NF' $p >/dev/null 2>&1 &"
  fi
  up "$p" && drive "$p"; kp "$p"
}

LV=/tmp/ob-clicknf-live; RP=/tmp/ob-clicknf-replay; CT=/tmp/ob-clicknf-ctl
echo "=== Software Click NF (OpenClickNP FlowCache+L4LB) — stateful-NF recovery (${GAP}s real gap) ==="
rm -f "$VB" "$VR"   # fresh base only before live; replay reuses the persisted base
run 9310 libos > "$LV"; sleep 1
echo ">>> kill -9 the NF; wait ${GAP}s (real wall clock advances); replay the packet log"
sleep "$GAP"
run 9311 libos > "$RP"; sleep 1
run 9312 control > "$CT"
echo "flows learned (live/replay/control): $(wc -l < "$LV")/$(wc -l < "$RP")/$(wc -l < "$CT")"
echo "live   table tail: $(tail -1 "$LV" 2>/dev/null)"
echo "replay table tail: $(tail -1 "$RP" 2>/dev/null)"
echo "control table tail: $(tail -1 "$CT" 2>/dev/null)   (real time → different last_seen)"
ident=no; diff -q "$LV" "$RP" >/dev/null 2>&1 && [ -s "$LV" ] && ident=yes
ctldiff=no; [ -s "$CT" ] && ! diff -q "$LV" "$CT" >/dev/null 2>&1 && ctldiff=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ]; then
  echo "RESULT: Click NF flow table (backend affinity + conntrack last-seen) byte-identical across recovery; control differs ✅"; exit 0
else
  echo "RESULT: Click NF determinism check FAILED (ident=$ident ctldiff=$ctldiff)"; diff "$LV" "$RP" | head; exit 1
fi
