#!/usr/bin/env bash
# OneBarrier — real RDMA verbs over SoftRoCE (rdma_rxe), upgrading the RDMA story
# from pure discrete-event simulation to REAL ibverbs measurement.
#
# The original 1Pipe operating point is real RDMA at 1-2 us RTT. We don't have RDMA
# hardware, but SoftRoCE (the in-kernel rdma_rxe soft-RoCE provider) runs the REAL
# ibverbs API over a commodity NIC. This script sets it up and measures:
#   * RC pingpong round-trip (ibv_rc_pingpong)
#   * RDMA_WRITE latency + the FT-overlap micro-benchmark (rdma_ftbench)
#
# Caveat (printed by the benchmark): SoftRoCE is CPU-bound, so it validates that the
# verbs path is real and that per-op latency sits in the 1-2 us regime, but the
# "FT is free" overlap is RTT-bound and is measured in ob-sim / needs real hardware.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
DEV=rxe0

echo "== set up SoftRoCE =="
sudo modprobe rdma_rxe 2>/dev/null
if ! rdma link show 2>/dev/null | grep -q "$DEV"; then
  NDEV=$(ip -o link show | awk -F': ' '$2!="lo" && $2!~"docker"{print $2; exit}')
  sudo rdma link add "$DEV" type rxe netdev "$NDEV" 2>/dev/null || true
fi
rdma link show 2>/dev/null | grep "$DEV" || { echo "rxe device not available"; exit 1; }
# pick a RoCE v2 gid index
GIDX=0
for g in 0 1 2 3; do
  [ "$(cat /sys/class/infiniband/$DEV/ports/1/gid_attrs/types/$g 2>/dev/null)" = "RoCE v2" ] && { GIDX=$g; break; }
done
echo "device=$DEV gid=$GIDX"

echo "== RC pingpong (real RDMA round-trip) =="
ibv_rc_pingpong -d "$DEV" -g "$GIDX" -n 1000 >/tmp/pp-s.log 2>&1 &
sleep 1
IP=$(ip -4 -o addr show "$(rdma link show $DEV 2>/dev/null|grep -oP 'netdev \K\S+')" 2>/dev/null | awk '{print $4}'|cut -d/ -f1)
timeout 30 ibv_rc_pingpong -d "$DEV" -g "$GIDX" -n 1000 "$IP" 2>&1 | grep -E 'usec/iter|Mbit'

echo "== FT-overlap micro-benchmark (rdma_ftbench) =="
[ -x "$HERE/rdma_ftbench" ] || gcc -O2 -o "$HERE/rdma_ftbench" "$HERE/rdma_ftbench.c" -libverbs
"$HERE/rdma_ftbench" "$DEV" "$GIDX"
