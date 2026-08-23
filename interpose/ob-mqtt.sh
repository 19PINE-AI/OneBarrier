#!/usr/bin/env bash
# OneBarrier — transparent FT for an MQTT message broker (Mosquitto).
#   ob-mqtt.sh [real_gap_seconds]
#
# A stock unmodified Mosquitto broker (single-threaded event loop, the IoT broker
# class) under the determinism libOS. Two recovered properties:
#   (1) STATE — the retained-message store (the broker's durable per-topic state)
#       reconstructs byte-identically by replaying the publish stream on a fresh
#       broker; a stateless restart loses it entirely.
#   (2) TIME — the broker uptime ($SYS/broker/uptime, derived from the wall clock) is
#       deterministic under the virtual clock (it counts input events), where a
#       no-libOS control's uptime tracks real time and differs.
# Publishes use QoS 1 (synchronous PUBACK) so the broker reads one message per
# read() — the per-event virtual-clock ticks are then deterministic over TCP.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=ob-common.sh
. "$HERE/ob-common.sh"
ob_require_shims libobpreload.so librngdet.so || exit 1
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
GAP="${1:-3}"; IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic

VB=/tmp/ob-mqtt-vb; VR=/tmp/ob-mqtt-vr
kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
conf(){ cat > /tmp/ob-mqtt-$1.conf <<EOF
listener $1 127.0.0.1
allow_anonymous true
sys_interval 1
persistence false
EOF
}
bcmd(){ conf "$1"; echo "mosquitto -c /tmp/ob-mqtt-$1.conf"; }
up(){ for i in $(seq 50); do mosquitto_sub -h 127.0.0.1 -p "$1" -t '$SYS/broker/version' -C 1 -W 1 >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }

# Workload: 8 retained messages (broker state) + a synchronous QoS-1 bulk to advance
# the clock; probe = retained store (sorted) + broker uptime.
drive(){ local p=$1
  for i in $(seq 0 7); do mosquitto_pub -h 127.0.0.1 -p "$p" -q 1 -t "sensors/$i" -m "v=$i" -r 2>/dev/null; done
  seq 1 1500 | mosquitto_pub -h 127.0.0.1 -p "$p" -q 1 -t bulk -l 2>/dev/null
  echo "uptime=$(mosquitto_sub -h 127.0.0.1 -p "$p" -t '$SYS/broker/uptime' -C 1 -W 3 2>/dev/null)"
  mosquitto_sub -h 127.0.0.1 -p "$p" -t 'sensors/#' -v -C 8 -W 3 2>/dev/null | sort
}

run(){ local p=$1 mode=$2; kp "$p"
  if [ "$mode" = libos ]; then
    eval "OPENSSL_ia32cap='$IA' OB_VCLOCK=$VB OB_VRAND=$VR LD_PRELOAD='$RNG $OBP' setarch -R $(bcmd "$p") >/dev/null 2>&1 &"
  else
    eval "$(bcmd "$p") >/dev/null 2>&1 &"
  fi
  up "$p" && drive "$p"; kp "$p"
}

LV=/tmp/ob-mqtt-live; RP=/tmp/ob-mqtt-replay; CT=/tmp/ob-mqtt-ctl
echo "=== Mosquitto MQTT broker — deterministic recovery (${GAP}s real gap) ==="
rm -f "$VB" "$VR"
run 1910 libos > "$LV"; sleep 1
echo ">>> kill -9 the broker; wait ${GAP}s; replay the publish stream on a fresh broker"; sleep "$GAP"
run 1911 libos > "$RP"; sleep 1
run 1912 control > "$CT"
echo "live    : $(head -1 "$LV")  | $(tail -1 "$LV")"
echo "replay  : $(head -1 "$RP")  | $(tail -1 "$RP")"
echo "control : $(head -1 "$CT")  | $(tail -1 "$CT")   (real time)"
ident=no; diff -q "$LV" "$RP" >/dev/null 2>&1 && [ -s "$LV" ] && ident=yes
ctldiff=no; [ -s "$CT" ] && ! diff -q "$LV" "$CT" >/dev/null 2>&1 && ctldiff=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ]; then
  echo "RESULT: Mosquitto retained store + broker uptime byte-identical across recovery; control differs ✅"; exit 0
else
  echo "RESULT: Mosquitto check FAILED (ident=$ident ctldiff=$ctldiff)"; diff "$LV" "$RP" | head; exit 1
fi
