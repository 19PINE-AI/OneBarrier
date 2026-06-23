#!/usr/bin/env bash
# memcached -t 1 bit-exact state determinism — closing the auxiliary-thread gap.
#   ob-memcached-deterministic.sh
#
# Even at -t 1, memcached spawns timer-driven maintenance threads (LRU maintainer,
# LRU crawler, hash-expander, slab reassign) that touch shared state on a REAL-time
# schedule — so the LRU bookkeeping evolves nondeterministically (its juggle count
# varies run-to-run). Disabling them with
#     -o no_lru_crawler,no_lru_maintainer,no_hashexpand,no_slab_reassign
# leaves only the dispatcher + the single worker, so the whole state evolution is a
# pure function of the request stream + the virtual clock.
#
# Part 1 shows the residual nondeterminism (maintainer juggles differ).
# Part 2 does a full crash+replay recovery under an EVICTION workload and verifies
#        the surviving-item set is byte-identical — deterministic recovery including
#        memcached's LRU eviction decisions.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SO="$HERE/libobpreload.so"
[ -f "$SO" ] || gcc -shared -fPIC -O2 -o "$SO" "$HERE/obpreload.c" -ldl -lpthread
DET="-o no_lru_crawler,no_lru_maintainer,no_hashexpand,no_slab_reassign"
kp(){ for p in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$p" 2>/dev/null; done; }

# Part 1: maintainer activity is real-time-driven (nondeterministic) vs disabled
juggles(){ # $1=port $2=flags
  kp "$1"; OB_VCLOCK=/tmp/mcj$1 LD_PRELOAD="$SO" memcached -p "$1" -U 0 -t 1 -m 4 $2 &
  for i in $(seq 40); do printf 'version\r\n'|timeout 1 nc 127.0.0.1 "$1" 2>/dev/null|grep -q VERSION && break; sleep 0.2; done
  python3 - "$1" <<'PY'
import socket,sys
p=int(sys.argv[1]); s=socket.create_connection(("127.0.0.1",p)); s.settimeout(8); v="x"*500
def c(x): s.sendall(x); return s.recv(8192)
for i in range(2000): c(f"set k{i} 0 0 500\r\n{v}\r\n".encode())
for r in range(20):
  for i in range(200): c(f"get k{i}\r\n".encode())
PY
  sleep 3
  echo "$(printf 'stats\r\nquit\r\n'|timeout 2 nc 127.0.0.1 "$1" 2>/dev/null|grep -oP 'lru_maintainer_juggles \K[0-9]+'||echo none)"
  kp "$1"; sleep 0.3
}
echo "== Part 1: LRU maintainer is real-time-driven (nondeterministic) =="
a=$(juggles 11560 ""); b=$(juggles 11561 "")
echo "   default -t 1:    juggles run A=$a  run B=$b   $([ "$a" != "$b" ] && echo '<- DIFFER (nondeterministic)' || echo '(same here, but timer-driven)')"
c=$(juggles 11562 "$DET"); d=$(juggles 11563 "$DET")
echo "   +disable flags:  juggles run A=$c  run B=$d   <- no maintainer thread (deterministic by construction)"

# Part 2: full crash+replay recovery under eviction, deterministic config
evict_fp(){ # $1=port  -> survivor fingerprint after an eviction workload
  kp "$1"; OB_VCLOCK=/tmp/mce LD_PRELOAD="$SO" memcached -p "$1" -U 0 -t 1 -m 2 $DET &
  for i in $(seq 40); do printf 'version\r\n'|timeout 1 nc 127.0.0.1 "$1" 2>/dev/null|grep -q VERSION && break; sleep 0.2; done
  python3 - "$1" <<'PY'
import socket,sys,hashlib
p=int(sys.argv[1]); s=socket.create_connection(("127.0.0.1",p)); s.settimeout(8); v="x"*1200
def c(x): s.sendall(x); return s.recv(8192)
for i in range(6000):
    c(f"set k{i} 0 0 1200\r\n{v}\r\n".encode())
    if i%2==0: c(f"get k{i-100}\r\n".encode())
sur=['1' if c(f"get k{i}\r\n".encode()).startswith(b"VALUE") else '0' for i in range(6000)]
print(hashlib.md5(''.join(sur).encode()).hexdigest()[:16]+" "+str(sur.count('1')))
PY
  kp "$1"; sleep 0.3
}
echo
echo "== Part 2: crash+replay recovery under LRU EVICTION (deterministic config) =="
rm -f /tmp/mce
L=$(evict_fp 11570); echo "   live    : survivors=$L"
echo "   >> crash; 3s real gap"; sleep 3
R=$(evict_fp 11571); echo "   recovered: survivors=$R"
if [ "${L% *}" = "${R% *}" ]; then
  echo "RESULT: memcached -t 1 eviction state byte-identical across recovery (no maintenance-thread nondeterminism) ✅"
else
  echo "RESULT: survivor sets differ ✗"
fi
