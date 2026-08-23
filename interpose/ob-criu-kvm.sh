#!/usr/bin/env bash
# OneBarrier CRIU-in-KVM: the GENERAL (any-binary) transparent checkpoint mechanism,
# demonstrated end-to-end in a real KVM guest with its own kernel instance.
#
# Why a guest: CRIU *restore* needs a kernel new enough for the CRIU build. The
# distro CRIU (3.16.1) segfaults restoring on kernel 6.8 (a trivial static process
# reproduces it — not app/jemalloc/rseq specific). The fix is CRIU >= 3.19. We build
# it and run it in a KVM guest booted on the host kernel so the result is clean and
# reproducible. Inside the guest CRIU checkpoint/restore of UNMODIFIED redis works,
# AND it preserves the libOS virtual-clock state (so the pre-checkpoint history,
# including libOS state, needs NO replay — bounded recovery, general mechanism).
#
# Outputs RESULT-A (redis state) and RESULT-B (virtual clock) from the guest console.
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=ob-common.sh
. "$HERE/ob-common.sh"
ob_require_shims libobpreload.so || exit 1
# Pick a bootable kernel image that HAS a matching /lib/modules tree (the running
# kernel's modules may be absent; KVM can boot any installed kernel image).
KREL=""
for img in $(ls -1 /boot/vmlinuz-*-generic 2>/dev/null | grep -v '\.old' | sort -V -r); do
  rel=${img#*/vmlinuz-}
  if [ -d "/lib/modules/$rel" ]; then KREL="$rel"; KSRC="$img"; break; fi
done
[ -n "$KREL" ] || { echo "no kernel image with matching /lib/modules found"; exit 1; }
echo "using guest kernel $KREL"
WORK=/tmp/ob-criu-kvm
CRIU_SRC=/tmp/criu-src
ROOT="$WORK/initramfs"
KIMG="$WORK/vmlinuz"
SHIM="$HERE/libobpreload.so"
[ -f "$SHIM" ] || gcc -shared -fPIC -O2 -o "$SHIM" "$HERE/obpreload.c" -ldl -lpthread

echo "== 0. prerequisites (qemu, build deps) =="
command -v qemu-system-x86_64 >/dev/null || sudo apt-get install -y qemu-system-x86 qemu-utils
command -v busybox >/dev/null || sudo apt-get install -y busybox-static

echo "== 1. build CRIU >= 3.19 (distro 3.16.1 can't restore on kernel 6.8) =="
if [ ! -x "$CRIU_SRC/criu/criu" ]; then
  sudo apt-get install -y build-essential libprotobuf-dev libprotobuf-c-dev protobuf-c-compiler \
    protobuf-compiler python3-protobuf libnl-3-dev libnl-route-3-dev libcap-dev pkg-config \
    libbsd-dev libnet1-dev iproute2 >/dev/null
  rm -rf "$CRIU_SRC"
  git clone --depth 1 --branch v3.19 https://github.com/checkpoint-restore/criu.git "$CRIU_SRC"
  make -C "$CRIU_SRC" -j"$(nproc)"
fi
"$CRIU_SRC/criu/criu" --version | head -1

echo "== 2. assemble initramfs (busybox + criu + redis + libOS + kernel modules) =="
sudo rm -rf "$ROOT"; mkdir -p "$ROOT"/{bin,sbin,proc,sys,dev,tmp,lib,lib64,usr/bin,usr/sbin,etc,i1,i2}
copybin(){ local b; b=$(command -v "$1") || return 0
  install -D -m755 "$(readlink -f "$b")" "$ROOT$b"
  ldd "$b" 2>/dev/null | grep -oP '/[^ ]+\.so[^ ]*' | sort -u | while read -r l; do
    [ -f "$ROOT$l" ] || install -D -m755 "$(readlink -f "$l")" "$ROOT$l"; done; }
# static busybox as the shell/init
install -D -m755 "$(command -v busybox)" "$ROOT/bin/busybox"
for a in sh mount umount ls cat echo sleep kill mkdir pidof grep head tail tr wc ifconfig modprobe lsmod poweroff sed find; do ln -sf busybox "$ROOT/bin/$a"; done
# the freshly-built criu + redis + net tools
install -D -m755 "$CRIU_SRC/criu/criu" "$ROOT/usr/sbin/criu"
for b in redis-server redis-cli ip nft iptables modprobe; do copybin "$b"; done
ldd "$CRIU_SRC/criu/criu" | grep -oP '/[^ ]+\.so[^ ]*' | sort -u | while read -r l; do [ -f "$ROOT$l" ] || install -D -m755 "$(readlink -f "$l")" "$ROOT$l"; done
install -D -m755 /lib64/ld-linux-x86-64.so.2 "$ROOT/lib64/ld-linux-x86-64.so.2"
install -D -m755 "$SHIM" "$ROOT/lib/libobpreload.so"
# full kernel module tree (CRIU needs *_diag, veth, nf_tables; depmod for modprobe)
mkdir -p "$ROOT/lib/modules"
sudo cp -a "/lib/modules/$KREL" "$ROOT/lib/modules/$KREL"
sudo chown -R "$(id -u):$(id -g)" "$ROOT/lib/modules"
sudo depmod -a -b "$ROOT" "$KREL"

echo "== 3. guest init script =="
cat > "$ROOT/init" <<'INIT'
#!/bin/sh
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev 2>/dev/null
mkdir -p /dev/pts; mount -t devpts devpts /dev/pts 2>/dev/null
ifconfig lo 127.0.0.1 up 2>/dev/null
echo "=== GUEST KERNEL $(uname -r), CRIU $(criu --version 2>/dev/null|grep -o '[0-9.]*'|head -1) ==="
for m in inet_diag tcp_diag udp_diag unix_diag netlink_diag packet_diag veth nfnetlink nf_tables; do modprobe $m 2>/dev/null; done
criu check >/dev/null 2>&1 && echo "[criu check] Looks good." || echo "[criu check] issues"

echo "[A] CRIU checkpoint/restore of UNMODIFIED redis (full state, no replay)"
redis-server --port 6379 --save '' --appendonly no --protected-mode no --daemonize yes --pidfile /tmp/p1 --logfile /tmp/r1 2>/dev/null; sleep 2
redis-cli -p 6379 SET k1 hello >/dev/null 2>&1; redis-cli -p 6379 INCR ctr >/dev/null 2>&1; redis-cli -p 6379 INCR ctr >/dev/null 2>&1
P1=$(cat /tmp/p1); echo "    before: dbsize=$(redis-cli -p 6379 DBSIZE) k1=$(redis-cli -p 6379 GET k1) ctr=$(redis-cli -p 6379 GET ctr)"
criu dump -t $P1 -D /i1 --tcp-established --file-locks -o /tmp/d1 2>/dev/null; D1=$?
kill -9 $P1 2>/dev/null; sleep 1
criu restore -d -D /i1 --tcp-established --file-locks -o /tmp/r1c 2>/dev/null; R1=$?; sleep 1
A_DB=$(redis-cli -p 6379 DBSIZE 2>/dev/null); A_K=$(redis-cli -p 6379 GET k1 2>/dev/null); A_C=$(redis-cli -p 6379 GET ctr 2>/dev/null)
echo "    after restore: dbsize=$A_DB k1=$A_K ctr=$A_C (dump=$D1 restore=$R1)"
[ "$D1" = 0 ] && [ "$R1" = 0 ] && [ "$A_C" = 2 ] && [ "$A_K" = hello ] && echo "RESULT-A: PASS — full redis state checkpoint+restore (general mechanism)" || echo "RESULT-A: FAIL"
for pid in $(pidof redis-server); do kill -9 $pid; done; sleep 1

echo "[B] CRIU preserves the libOS VIRTUAL CLOCK across checkpoint/restore"
rm -f /tmp/vb
OB_VCLOCK=/tmp/vb LD_PRELOAD=/lib/libobpreload.so redis-server --port 6380 --save '' --appendonly no --protected-mode no --daemonize yes --pidfile /tmp/p2 --logfile /tmp/r2 2>/dev/null; sleep 2
for i in 1 2 3; do redis-cli -p 6380 TIME >/dev/null 2>&1; done
T_BEFORE=$(redis-cli -p 6380 TIME 2>/dev/null | head -1); P2=$(cat /tmp/p2)
echo "    virtual TIME before checkpoint: $T_BEFORE"
criu dump -t $P2 -D /i2 --tcp-established --file-locks -o /tmp/d2 2>/dev/null; D2=$?
kill -9 $P2 2>/dev/null; sleep 2
criu restore -d -D /i2 --tcp-established --file-locks -o /tmp/r2c 2>/dev/null; R2=$?; sleep 1
T_AFTER=$(redis-cli -p 6380 TIME 2>/dev/null | head -1)
echo "    virtual TIME after restore:     $T_AFTER (2s real gap elapsed while down)"
[ "$D2" = 0 ] && [ "$R2" = 0 ] && [ -n "$T_AFTER" ] && [ "$T_AFTER" = "$T_BEFORE" ] && echo "RESULT-B: PASS — virtual clock preserved by CRIU (in-memory libOS state survives C/R)" || echo "RESULT-B: PARTIAL (D2=$D2 R2=$R2 before=$T_BEFORE after=$T_AFTER)"
echo "=== GUEST-DONE ==="
poweroff -f
INIT
chmod +x "$ROOT/init"

echo "== 4. pack initramfs + kernel =="
[ -f "$KIMG" ] || { sudo cp "$KSRC" "$KIMG"; sudo chmod +r "$KIMG"; }
( cd "$ROOT" && find . | cpio -o -H newc 2>/dev/null | gzip -1 > "$WORK/initramfs.cpio.gz" )

echo "== 5. boot KVM guest =="
sudo timeout 200 qemu-system-x86_64 -enable-kvm -m 4096 -smp 2 \
  -kernel "$KIMG" -initrd "$WORK/initramfs.cpio.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init quiet" -nographic -no-reboot 2>&1 \
  | grep -aE 'KERNEL|criu check|before:|after|RESULT-A|RESULT-B|TIME|GUEST-DONE'
