#!/usr/bin/env bash
# OneBarrier — deterministic-replay recovery of an UNMODIFIED SQL database.
#   ob-sqlite.sh [real_gap_seconds]
#
# This is the first DATABASE result on the order-log-free deterministic-replay path
# (the Postgres result is CRIU checkpoint-only; a multi-process shared-everything DB
# is not a replay candidate). SQLite is single-threaded and embeddable, so a stock
# python3 process using the stdlib `sqlite3` module (which links libsqlite3) recovers
# byte-identically by deterministic replay under the libOS.
#
# Non-determinism closed: (1) TIME — SQLite `strftime('now')` reads gettimeofday,
# caught by the virtual clock; (2) RANDOMNESS — SQLite's PRNG (`random()`,
# `randomblob()`) seeds once from /dev/urandom, redirected to a deterministic file via
# a private mount namespace (same mechanism as redis SipHash). A fresh in-memory DB
# replays the same request sequence; live == replay; a no-libOS control (real
# time + real urandom) differs in BOTH the timestamps and the random ids.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"; DET=/tmp/ob-det-urandom
GAP="${1:-3}"; IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic

# Deterministic /dev/urandom replacement (fixed seed ⇒ identical every run).
[ -f "$DET" ] || python3 - "$DET" <<'PY'
import struct, sys
s=0x0B1A2C3D4E5F6071; out=bytearray()
for _ in range((1<<20)>>3):
    s=(s+0x9E3779B97F4A7C15)&0xFFFFFFFFFFFFFFFF; z=s
    z=((z^(z>>30))*0xBF58476D1CE4E5B9)&0xFFFFFFFFFFFFFFFF
    z=((z^(z>>27))*0x94D049BB133111EB)&0xFFFFFFFFFFFFFFFF
    z^=(z>>31); out+=struct.pack('<Q',z)
open(sys.argv[1],'wb').write(out)
PY

# An UNMODIFIED stock server: stdlib http.server + sqlite3. Each GET inserts a row
# whose values come from SQLite's own time + RNG, then returns the full table. We do
# not touch SQLite internals — the determinism comes entirely from the libOS.
SRV=/tmp/ob-sqlite-srv.py
cat > "$SRV" <<'PY'
import sqlite3, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
db = sqlite3.connect(":memory:")
db.execute("CREATE TABLE t(id INTEGER PRIMARY KEY, ts TEXT, rnd TEXT)")
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        # ts: sub-second wall time (gettimeofday via the virtual clock).
        # rnd: SQLite PRNG (seeded from /dev/urandom — redirected deterministically).
        db.execute("INSERT INTO t(ts,rnd) VALUES (strftime('%s.%f','now'), lower(hex(randomblob(6))))")
        db.commit()
        rows = db.execute("SELECT id,ts,rnd FROM t ORDER BY id").fetchall()
        body = "".join(f"{i}|{ts}|{r}\n" for (i,ts,r) in rows).encode()
        self.send_response(200); self.send_header("Content-Length", str(len(body))); self.end_headers()
        self.wfile.write(body)
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY

PYENV="PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1"
VB=/tmp/ob-sqlite-vb; VR=/tmp/ob-sqlite-vr
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
up(){ for i in $(seq 50); do curl -s --max-time 1 "127.0.0.1:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
# Drive 8 inserts; the probe is the final table (ts + rnd columns).
drive(){ local p=$1 last=""; for i in $(seq 1 8); do last=$(curl -s --max-time 2 "127.0.0.1:$p/" 2>/dev/null); done; echo "$last"; }

# $1=port  $2=mode(libos|control). The server runs in the FOREGROUND inside the
# (back-grounded) wrapper so the mount namespace stays alive while it serves; an
# inner `& sleep` double-background drops the child in some sandboxes.
run(){ local p=$1 mode=$2; kp "$p"
  if [ "$mode" = libos ]; then
    unshare -r -m bash -c "mount --bind $DET /dev/urandom; OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' $PYENV setarch -R python3 $SRV $p >/dev/null 2>&1" &
  else
    eval "$PYENV python3 $SRV $p >/dev/null 2>&1 &"
  fi
  up "$p" && drive "$p"; kp "$p"
}

LV=/tmp/ob-sqlite-live; RP=/tmp/ob-sqlite-replay; CT=/tmp/ob-sqlite-ctl
echo "=== SQLite (unmodified python3 + stdlib sqlite3) — deterministic-replay recovery (${GAP}s real gap) ==="
rm -f "$VB" "$VR"   # fresh base ONLY before the live run; replay reuses the persisted base
run 8410 libos > "$LV"; sleep 1
echo ">>> crash; wait ${GAP}s so the real wall clock advances"; sleep "$GAP"
run 8411 libos > "$RP"; sleep 1
run 8412 control > "$CT"
echo "live    : $(tail -2 "$LV" 2>/dev/null | tr '\n' ' ')"
echo "replay  : $(tail -2 "$RP" 2>/dev/null | tr '\n' ' ')"
echo "control : $(tail -2 "$CT" 2>/dev/null | tr '\n' ' ')   (real time + real urandom)"
ident=no; diff -q "$LV" "$RP" >/dev/null 2>&1 && [ -s "$LV" ] && ident=yes
ctldiff=no; [ -s "$CT" ] && ! diff -q "$LV" "$CT" >/dev/null 2>&1 && ctldiff=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ]; then
  echo "RESULT: SQLite DB state (time- AND RNG-derived rows) byte-identical across recovery; control differs ✅"; exit 0
else
  echo "RESULT: SQLite determinism check FAILED (ident=$ident ctldiff=$ctldiff)"; diff "$LV" "$RP" | head; exit 1
fi
