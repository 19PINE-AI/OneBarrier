#!/usr/bin/env bash
# OneBarrier — transparent FT for a DNS resolver (dnsmasq).
#   ob-dns.sh [real_gap_seconds]
#
# A stock unmodified dnsmasq (single-threaded) caching resolver under the libOS. Its
# observable time-dependent state is the cached record TTL, which counts DOWN as the
# clock advances: a cached answer returns remaining-TTL = original - elapsed. Under
# the virtual clock "elapsed" counts input events (DNS queries over TCP, which the
# libOS ticks on accept()+read()), so the remaining TTL is a deterministic function
# of the query sequence: a recovered resolver re-derives the EXACT remaining TTL it
# had before the crash, where a no-libOS control's TTL tracks real wall-clock time.
# (UDP is not ticked, so queries use DNS-over-TCP.)
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=ob-common.sh
. "$HERE/ob-common.sh"
ob_require_shims libobpreload.so librngdet.so || exit 1
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
GAP="${1:-3}"; IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic
VB=/tmp/ob-dns-vb; VR=/tmp/ob-dns-vr
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done
      for pid in $(ss -ulnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

# fast DNS-over-UDP client: query test.local N times; on the last, print the TTL.
# UDP (recvmsg in dnsmasq's main process, no per-query fork) is what the libOS ticks.
client(){ python3 - "$1" "$2" <<'PY'
import socket, struct, sys
port=int(sys.argv[1]); n=int(sys.argv[2])
q=struct.pack(">H",0x1234)+struct.pack(">H",0x0100)+struct.pack(">HHHH",1,0,0,0)
for lbl in ("test","local"): q+=bytes([len(lbl)])+lbl.encode()
q+=b"\x00"+struct.pack(">HH",1,1)
ttl=None
for i in range(n):
    s=socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.settimeout(5)
    s.sendto(q,("127.0.0.1",port))
    b,_=s.recvfrom(4096); s.close()
    if i==n-1:
        off=12
        while b[off]!=0: off+=b[off]+1
        off+=5                      # end of QNAME + QTYPE + QCLASS
        ttl=struct.unpack(">I", b[off+6:off+10])[0]   # answer ttl
print(ttl)
PY
}

run(){ local p=$1 mode=$2; kp "$p"
  local cache="dnsmasq --port=$p --no-resolv --no-hosts --server=/test.local/127.0.0.1#5301 --cache-size=2000 --keep-in-foreground --log-facility=/tmp/ob-dns-$p.log"
  if [ "$mode" = libos ]; then
    eval "OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' setarch -R $cache >/dev/null 2>&1 &"
  else
    eval "$cache >/dev/null 2>&1 &"
  fi
  for i in $(seq 50); do client "$p" 1 >/dev/null 2>&1 && break; sleep 0.2; done
  client "$p" 2500          # 2500 TCP queries; advances ~2.5 s of virtual time; prints final TTL
  kp "$p"
}

# Shared authoritative upstream (plain, real time): test.local -> 10.1.2.3, TTL 3600.
pkill -9 dnsmasq 2>/dev/null; sleep 0.5
dnsmasq --port=5301 --no-resolv --no-hosts --host-record=test.local,10.1.2.3 --local-ttl=3600 \
        --keep-in-foreground --log-facility=/tmp/ob-dns-up.log >/dev/null 2>&1 &
sleep 1

echo "=== dnsmasq DNS resolver — deterministic TTL recovery (${GAP}s real gap) ==="
rm -f "$VB" "$VR"
L=$(run 5310 libos); sleep 1
echo ">>> kill -9 the resolver; wait ${GAP}s; replay the query stream on a fresh resolver"; sleep "$GAP"
R=$(run 5311 libos); sleep 1
C=$(run 5312 control)
pkill -9 dnsmasq 2>/dev/null
echo "live remaining-TTL   : $L"
echo "replay remaining-TTL : $R"
echo "control remaining-TTL: $C   (real time)"
if [ -n "$L" ] && [ "$L" = "$R" ] && [ "$L" != "$C" ]; then
  echo "RESULT: dnsmasq cached-record TTL byte-identical across recovery ($L), control differs ($C) ✅"; exit 0
else
  echo "RESULT: dnsmasq check FAILED (live=$L replay=$R control=$C)"; exit 1
fi
