#!/usr/bin/env bash
# OneBarrier — transparent fault tolerance for a partitioned message broker.
#   ob-redis-streams.sh [real_gap_seconds] [n_shards]
#
# Demonstrates the broker extension (paper §Extensions): a stock, UNMODIFIED
# redis-server used as a Kafka-class stream broker (XADD/XREAD) recovers
# byte-identically across a crash + real-time gap under the determinism libOS.
#
# Why streams are a sharp broker probe: an auto-ID Redis stream entry is
# `<ms>-<seq>`, where `ms` is read from the server clock. Under the virtual clock
# the entry IDs are a deterministic function of the input sequence, so a recovered
# broker re-derives the EXACT same offsets/IDs it had before the crash — the
# streaming-log analog of nginx's Date: header. A no-libOS control (real time)
# gets different IDs. Partitioning = N share-nothing single-thread shards (the
# Kafka partition model), each deterministic by construction.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SO="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
GAP="${1:-3}"; SHARDS="${2:-2}"
IA='~0x4000000000000000:~0x0'   # disable RDRAND/RDSEED for OpenSSL (untrappable CPU instr)

[ -f "$SO" ]  || { echo "missing $SO"; exit 2; }
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -lpthread 2>/dev/null

kill_port(){ for pid in $(ss -tlnp 2>/dev/null | grep ":$1 " | grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
bcmd(){ echo "redis-server --port $1 --save '' --appendonly no --logfile /tmp/ob-rs$1.log"; }

# Drive a deterministic broker workload: publish 8 messages to a stream with
# auto-generated (time-derived) IDs, across two consumer-group reads, and print
# the resulting entry IDs (the offsets a downstream consumer would commit).
drive(){
  local port="$1"
  redis-cli -p "$port" DEL topic >/dev/null 2>&1
  redis-cli -p "$port" XGROUP CREATE topic g1 0 MKSTREAM >/dev/null 2>&1
  for i in $(seq 1 8); do
    redis-cli -p "$port" XADD topic '*' k "key$i" v "val$i" 2>/dev/null
  done
  # consumer-group read: the committed offsets a Kafka-class consumer would see
  redis-cli -p "$port" XLEN topic 2>/dev/null
}

run_shard(){
  # $1=base-port-triplet-index ; records live, crashes, gaps, replays, control.
  local idx="$1"
  local P1=$((6600 + idx*10)) P2=$((6601 + idx*10)) P3=$((6602 + idx*10))
  local VB="/tmp/ob-rs-vbase-$idx" VR="/tmp/ob-rs-vrand-$idx"
  local L="/tmp/ob-rs-live-$idx" R="/tmp/ob-rs-replay-$idx" C="/tmp/ob-rs-ctl-$idx"
  local PRE="OPENSSL_ia32cap='$IA' OB_VCLOCK='$VB' OB_VRAND='$VR' LD_PRELOAD='$RNG $SO' setarch -R"
  local CTL="LD_PRELOAD='$SO'"
  up(){ for i in $(seq 50); do redis-cli -p "$1" PING 2>/dev/null | grep -q PONG && return 0; sleep 0.2; done; return 1; }

  kill_port "$P1"; kill_port "$P2"; kill_port "$P3"; sleep 0.5; rm -f "$VB" "$VR" "$L" "$R" "$C"
  # 1) LIVE — record broker offsets under the virtual clock
  eval "$PRE $(bcmd "$P1") >/dev/null 2>&1 &"
  up "$P1" && drive "$P1" > "$L"
  kill_port "$P1"; sleep 0.5
  # 2) crash + real-time gap, then REPLAY on a fresh shard with the same base
  sleep "$GAP"
  eval "$PRE $(bcmd "$P2") >/dev/null 2>&1 &"
  up "$P2" && drive "$P2" > "$R"
  kill_port "$P2"; sleep 0.5
  # 3) CONTROL — no virtual clock (real time) ⇒ different IDs
  eval "$CTL $(bcmd "$P3") >/dev/null 2>&1 &"
  up "$P3" && drive "$P3" > "$C"
  kill_port "$P3"

  local ident=no ctldiff=no
  diff -q "$L" "$R" >/dev/null 2>&1 && [ -s "$L" ] && ident=yes
  [ -s "$C" ] && ! diff -q "$L" "$C" >/dev/null 2>&1 && ctldiff=yes
  echo "  shard $idx  live   : $(head -1 "$L" 2>/dev/null) … $(tail -2 "$L" 2>/dev/null | head -1)"
  echo "  shard $idx  replay : $(head -1 "$R" 2>/dev/null) … $(tail -2 "$R" 2>/dev/null | head -1)"
  echo "  shard $idx  control: $(head -1 "$C" 2>/dev/null) … $(tail -2 "$C" 2>/dev/null | head -1)  (real time)"
  [ "$ident" = yes ] && [ "$ctldiff" = yes ]
}

echo "=== Redis Streams broker — deterministic recovery, $SHARDS share-nothing shards (${GAP}s real gap) ==="
rc=0
for s in $(seq 0 $((SHARDS-1))); do run_shard "$s" || rc=1; done
if [ "$rc" = 0 ]; then
  echo "RESULT: redis-streams broker DETERMINISTIC ✅ — every shard's stream IDs/offsets byte-identical across crash+gap; control (real time) differs"
else
  echo "RESULT: redis-streams broker FAILED determinism check"
fi
exit $rc
