#!/usr/bin/env bash
# OneBarrier checkpoint + tail-replay recovery for an UNMODIFIED app.
#   ob-checkpoint-replay.sh
#
# Recovery by replaying the request stream from process start costs O(total
# requests). A checkpoint bounds it to O(tail): restore an app-native snapshot
# (here redis RDB) for the pre-checkpoint state, and replay only the requests
# AFTER the checkpoint. The virtual clock resumes from the checkpoint's tick count
# (OB_VCLOCK_TICKS), so the tail's time-derived state (TTLs) stays exact.
#
# CRIU would be the general (any-binary) checkpoint mechanism; it is unavailable in
# this sandbox (no CAP_SYS_ADMIN / netns), so we use redis's native RDB to
# demonstrate the same principle. The OneBarrier ENGINE quantifies the identical
# checkpoint-vs-replay tradeoff in RQ8 (STATUS.md).
#
# Demonstrates: checkpoint-recovery replays only the TAIL yet reconstructs state
# byte-identical to the live run (TTLs included) and to a full-replay-from-start.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OBP="$HERE/libobpreload.so"
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }

VB=/tmp/cr-vb; TK=/tmp/cr-ticks; rm -f "$VB" "$TK"
CKDIR=/tmp/cr-ck; rm -rf "$CKDIR"; mkdir -p "$CKDIR"
PRE="OB_VCLOCK=$VB OB_VCLOCK_TICKS=$TK LD_PRELOAD='$OBP'"

# readiness via the LISTEN socket only — sending a redis command here would be an
# extra marked-connection read that advances the virtual clock and would not be
# reproduced identically across full vs checkpoint recovery. redis listens only
# after it finishes loading the RDB, so "listening" ⇒ ready.
up(){ for i in $(seq 60); do ss -tln 2>/dev/null|grep -q ":$1 " && return 0; sleep 0.2; done; return 1; }
state(){ for k in $(redis-cli -p "$1" KEYS '*'|sort); do echo "$k=$(redis-cli -p "$1" GET "$k") pttl=$(redis-cli -p "$1" PTTL "$k")"; done; }
part1(){ for i in $(seq 1 20); do redis-cli -p "$1" SET k$i v$i EX $((1000+i)) >/dev/null; done; }
part2(){ for i in $(seq 21 40); do redis-cli -p "$1" SET k$i v$i EX $((1000+i)) >/dev/null; done; }

launch(){ kp "$1"; eval "$2 redis-server --port $1 --save '' --appendonly no --dir $3 --dbfilename dump.rdb --logfile /tmp/cr$1.log &"; up "$1"; }

echo "=== LIVE: 40 SETs with TTLs, SAVE checkpoint after the first 20 ==="
launch 6630 "$PRE" "$CKDIR"
part1 6630
redis-cli -p 6630 SAVE >/dev/null            # checkpoint marker (part of the request stream)
cp "$CKDIR/dump.rdb" "$CKDIR/ckpt.rdb"        # snapshot state @ checkpoint
cp "$TK" "$CKDIR/ckpt.ticks"                  # snapshot virtual-clock tick @ checkpoint
part2 6630
state 6630 > /tmp/cr-live.txt
echo "live state: $(wc -l </tmp/cr-live.txt) keys; checkpoint tick=$(od -An -tu8 "$CKDIR/ckpt.ticks"|tr -d ' ')"
kp 6630; echo ">> crash"; sleep 2

echo
echo "=== RECOVER-FULL (baseline): replay ALL 41 requests from process start ==="
rm -rf /tmp/cr-full; mkdir -p /tmp/cr-full
launch 6631 "OB_VCLOCK=$VB LD_PRELOAD='$OBP'" /tmp/cr-full
part1 6631; redis-cli -p 6631 SAVE >/dev/null; part2 6631   # replay the full stream incl SAVE
state 6631 > /tmp/cr-full.txt
echo "replayed: 41 requests (20 + SAVE + 20)"
kp 6631

echo
echo "=== RECOVER-CKPT: restore RDB snapshot + resume tick, replay ONLY the 20-request tail ==="
rm -rf /tmp/cr-ck2; mkdir -p /tmp/cr-ck2; cp "$CKDIR/ckpt.rdb" /tmp/cr-ck2/dump.rdb
launch 6632 "OB_VCLOCK=$VB OB_VCLOCK_TICKS=$CKDIR/ckpt.ticks LD_PRELOAD='$OBP'" /tmp/cr-ck2
part2 6632                                     # tail only — pre-checkpoint keys come from the RDB
state 6632 > /tmp/cr-ck.txt
echo "replayed: 20 requests (tail only) — pre-checkpoint state from RDB"
kp 6632

echo
echo "--- verification ---"
echo "live      keys=$(wc -l </tmp/cr-live.txt)  sample: $(head -1 /tmp/cr-live.txt)"
echo "full      keys=$(wc -l </tmp/cr-full.txt)  sample: $(head -1 /tmp/cr-full.txt)"
echo "ckpt+tail keys=$(wc -l </tmp/cr-ck.txt)  sample: $(head -1 /tmp/cr-ck.txt)"
ok=1
diff -q /tmp/cr-live.txt /tmp/cr-ck.txt   >/dev/null || ok=0
diff -q /tmp/cr-live.txt /tmp/cr-full.txt >/dev/null || ok=0
if [ $ok = 1 ]; then
  echo "RESULT: checkpoint+tail-replay reconstructs IDENTICAL state replaying 20 reqs vs 41 (2.05x less) ✅"
else
  echo "RESULT: state mismatch ✗"
  echo "ckpt vs live:"; diff /tmp/cr-live.txt /tmp/cr-ck.txt | head
  echo "full vs live:"; diff /tmp/cr-live.txt /tmp/cr-full.txt | head
fi
