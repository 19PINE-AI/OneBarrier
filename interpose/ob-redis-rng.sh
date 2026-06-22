#!/usr/bin/env bash
# Deterministic recovery of redis-INTERNAL RNG state (SPOP/SRANDMEMBER).
#   ob-redis-rng.sh
#
# Redis 6 seeds its dict (SipHash) from /dev/urandom — read directly, bypassing
# both the getrandom(2) seccomp trap and LD_PRELOAD symbol interposition (glibc
# fopen uses an internal openat). So commands whose result depends on the dict
# layout / PRNG (SPOP, SRANDMEMBER) are nondeterministic across restarts and could
# not be reconstructed on recovery.
#
# Fix: run redis in a private MOUNT namespace with a deterministic file bind-mounted
# over /dev/urandom (so the SipHash seed is fixed), combined with the rest of the
# libOS stack (virtual clock + getrandom trap + ASLR-off + no-RDRAND). Then the
# RNG-derived state is reproducible, so deterministic-replay recovery rebuilds it
# byte-identically. A control without the redirect rebuilds DIFFERENT state.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"; DET=/tmp/ob-det-urandom
IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic

# deterministic /dev/urandom replacement (fixed seed ⇒ identical every run)
python3 - "$DET" <<'PY'
import struct, sys
s=0x0B1A2C3D4E5F6071; out=bytearray()
for _ in range((1<<20)>>3):
    s=(s+0x9E3779B97F4A7C15)&0xFFFFFFFFFFFFFFFF; z=s
    z=((z^(z>>30))*0xBF58476D1CE4E5B9)&0xFFFFFFFFFFFFFFFF
    z=((z^(z>>27))*0x94D049BB133111EB)&0xFFFFFFFFFFFFFFFF
    z^= (z>>31); out+=struct.pack('<Q',z)
open(sys.argv[1],'wb').write(out)
PY

VB=/tmp/ob-rr-vb; VR=/tmp/ob-rr-vr; rm -f "$VB" "$VR"
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

# RNG-derived workload: build a set, SPOP a random subset; the remaining members and
# the popped subset depend entirely on redis's internal RNG.
workload(){ local p=$1
  redis-cli -p "$p" DEL big popped >/dev/null
  for i in $(seq 1 40); do redis-cli -p "$p" SADD big m$i >/dev/null; done
  redis-cli -p "$p" SPOP big 15 | while read m; do redis-cli -p "$p" RPUSH popped "$m" >/dev/null; done
}
state(){ local p=$1; echo "remaining=$(redis-cli -p "$p" SMEMBERS big|sort|tr '\n' ',')"; echo "popped=$(redis-cli -p "$p" LRANGE popped 0 -1|tr '\n' ',')"; }

# $1=port  $2=redirect(1=mount-ns det /dev/urandom, 0=real)
run(){ local p=$1 redir=$2; kp "$p"
  local launch="OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' setarch -R redis-server --port $p --save '' --appendonly no --logfile /tmp/ob-rr$p.log"
  if [ "$redir" = 1 ]; then
    unshare -r -m bash -c "mount --bind $DET /dev/urandom; $launch & sleep 5" &
  else
    eval "$launch & sleep 5" &
  fi
  for i in $(seq 30); do redis-cli -p "$p" PING 2>/dev/null|grep -q PONG && break; sleep 0.2; done
  workload "$p"; state "$p"; kp "$p"
}

echo "=== LIVE (libOS: deterministic /dev/urandom) ==="
run 6760 1 > /tmp/ob-rr-live.txt; cat /tmp/ob-rr-live.txt
echo ">> crash; 3s gap"; sleep 3
echo "=== RECOVERED (replay workload, same seed+clock+urandom) ==="
run 6761 1 > /tmp/ob-rr-rec.txt; cat /tmp/ob-rr-rec.txt
echo "=== CONTROL (real /dev/urandom — RNG state must differ) ==="
run 6762 0 > /tmp/ob-rr-ctl.txt; cat /tmp/ob-rr-ctl.txt

echo
if diff -q /tmp/ob-rr-live.txt /tmp/ob-rr-rec.txt >/dev/null && ! diff -q /tmp/ob-rr-live.txt /tmp/ob-rr-ctl.txt >/dev/null; then
  echo "RESULT: redis RNG-derived state (SPOP/remaining) byte-identical across recovery, control differs ✅"
else
  echo "RESULT: mismatch ✗"; echo "rec diff:"; diff /tmp/ob-rr-live.txt /tmp/ob-rr-rec.txt|head
fi
