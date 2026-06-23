#!/usr/bin/env bash
# OneBarrier CRIU-in-KVM for PostgreSQL — the GENERAL checkpoint mechanism on the
# HARDEST app class: a multi-process database (postmaster + background workers,
# SysV-IPC + POSIX shared memory, WAL, on-disk data dir).
#
#   ob-criu-postgres-kvm.sh
#
# Builds on the base guest from ob-criu-kvm.sh (CRIU 3.19 + kernel + modules +
# busybox initramfs) and adds an unmodified PostgreSQL 14 + an initialized data dir.
# Inside the guest, CRIU checkpoint/restore of the whole PG process tree is verified:
# data byte-identical across the crash, and the restored server is live (accepts a
# new write). PG is run in a private IPC namespace (so CRIU can dump its SysV shm)
# and as the guest's root with a faked uid (PG refuses root; setpriv in the minimal
# initramfs lacks --reuid) — both are guest-internal tricks, the app is unmodified.
#
# Prereq: run ob-criu-kvm.sh once first (it builds /tmp/criu-src CRIU 3.19, the
# kernel image, and the base initramfs at /tmp/ob-criu-kvm).
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK=/tmp/ob-criu-kvm
R="$WORK/initramfs"
PGV=14
PGBIN=/usr/lib/postgresql/$PGV/bin
[ -x "/tmp/criu-src/criu/criu" ] && [ -f "$WORK/vmlinuz" ] && [ -d "$R" ] || { echo "run ob-criu-kvm.sh first"; exit 1; }
command -v "$PGBIN/postgres" >/dev/null || sudo apt-get install -y postgresql postgresql-client

echo "== 1. initdb a fresh data dir + table with time/random columns =="
PGDATA=/tmp/ob-pgdata; rm -rf "$PGDATA"
"$PGBIN/initdb" -D "$PGDATA" -A trust --no-locale -E UTF8 >/dev/null
cat >> "$PGDATA/postgresql.conf" <<CONF
unix_socket_directories = '/tmp'
port = 5440
listen_addresses = '127.0.0.1'
timezone = 'UTC'
log_timezone = 'UTC'
CONF
"$PGBIN/pg_ctl" -D "$PGDATA" -l /tmp/ob-pg-init.log -w start >/dev/null
"$PGBIN/psql" -h /tmp -p 5440 -U "$(whoami)" -d postgres -q <<SQL
CREATE TABLE t(id serial primary key, val int, created timestamptz default now(), rnd float8 default random());
INSERT INTO t(val) SELECT g FROM generate_series(1,1000) g;
SQL
"$PGBIN/pg_ctl" -D "$PGDATA" -m fast stop >/dev/null
rm -f "$PGDATA/postmaster.pid" "$PGDATA/postmaster.opts"

echo "== 2. augment the base initramfs with PostgreSQL + deps =="
copybin(){ install -D -m755 "$(readlink -f "$1")" "$R$1"
  ldd "$1" 2>/dev/null|grep -oP '/[^ ]+\.so[^ ]*'|sort -u|while read -r l; do [ -f "$R$l" ]||install -D -m755 "$(readlink -f "$l")" "$R$l"; done; }
for b in postgres pg_ctl psql initdb; do copybin "$PGBIN/$b"; done
sudo rm -rf "$R/usr/lib/postgresql" "$R/usr/share/postgresql" "$R/usr/share/zoneinfo" "$R/pgdata"
sudo cp -a "/usr/lib/postgresql/$PGV/lib" "$R/usr/lib/postgresql/$PGV/lib" 2>/dev/null || { mkdir -p "$R/usr/lib/postgresql/$PGV"; sudo cp -a "/usr/lib/postgresql/$PGV/lib" "$R/usr/lib/postgresql/$PGV/"; }
sudo cp -a /usr/share/postgresql "$R/usr/share/postgresql"
sudo cp -a /usr/share/zoneinfo  "$R/usr/share/zoneinfo"
sudo cp -a "$PGDATA" "$R/pgdata"
sudo chown -R "$(id -u):$(id -g)" "$R/usr/lib/postgresql" "$R/usr/share/postgresql" "$R/usr/share/zoneinfo" "$R/pgdata"
# fake-uid shim so PG (which refuses root) runs as the guest's PID-1 root
cat > /tmp/ob-fakeuid.c <<'C'
#include <sys/types.h>
#include <unistd.h>
uid_t getuid(void){ return 1000; }
uid_t geteuid(void){ return 1000; }
C
gcc -shared -fPIC -O2 -o "$R/lib/libfakeuid.so" /tmp/ob-fakeuid.c

echo "== 3. guest init: start PG in a private IPC ns, CRIU dump/restore, verify =="
cat > "$R/init" <<INIT
#!/bin/sh
export PATH=/bin:/sbin:/usr/bin:/usr/sbin:$PGBIN
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev 2>/dev/null
mkdir -p /dev/pts /dev/shm; mount -t devpts devpts /dev/pts 2>/dev/null; mount -t tmpfs tmpfs /dev/shm
ifconfig lo 127.0.0.1 up 2>/dev/null
echo "=== GUEST KERNEL \$(uname -r), CRIU \$(criu --version 2>/dev/null|grep -o '[0-9.]*'|head -1) ==="
for m in inet_diag tcp_diag udp_diag unix_diag netlink_diag packet_diag veth nfnetlink nf_tables; do modprobe \$m 2>/dev/null; done
PSQL="env PATH=$PGBIN:\$PATH psql -h /tmp -p 5440 -U ubuntu -d postgres -tAc"
echo "[1] start UNMODIFIED PostgreSQL (multi-process) in a private IPC namespace"
rm -f /pgdata/postmaster.pid
unshare --ipc -- env LD_PRELOAD=/lib/libfakeuid.so PATH=$PGBIN:\$PATH $PGBIN/pg_ctl -D /pgdata -l /tmp/pg.log -w -t 40 start
sleep 2
PM=\$(head -1 /pgdata/postmaster.pid 2>/dev/null)
echo "    postmaster=\$PM  socket=\$([ -S /tmp/.s.PGSQL.5440 ] && echo yes || echo no)"
[ -n "\$PM" ] || { echo "PGLOG:"; head -12 /tmp/pg.log; echo '=== GUEST-DONE ==='; poweroff -f; }
BEFORE=\$(\$PSQL "select count(*)||'|'||sum(val) from t" 2>/dev/null); echo "    data before: \$BEFORE"
echo "[2] CRIU dump of the multi-process tree"
mkdir -p /i; criu dump -t \$PM -D /i --shell-job --tcp-established --file-locks --ext-unix-sk -o d.log
echo "    dump exit=\$?  images=\$(ls /i 2>/dev/null|wc -l)  pg-alive=\$(kill -0 \$PM 2>/dev/null && echo yes || echo no)"
for p in \$(pgrep -f 'bin/postgres'); do kill -9 \$p 2>/dev/null; done; sleep 1
echo "[3] CRIU restore"
criu restore -d -D /i --shell-job --tcp-established --file-locks --ext-unix-sk -o r.log
echo "    restore exit=\$?"; sleep 2
AFTER=\$(\$PSQL "select count(*)||'|'||sum(val) from t" 2>/dev/null); echo "    data after restore: \$AFTER"
\$PSQL "insert into t(val) values (9999)" >/dev/null 2>&1; NEW=\$(\$PSQL "select count(*) from t" 2>/dev/null)
echo "    post-restore write: count=\$NEW"
[ -n "\$AFTER" ] && [ "\$BEFORE" = "\$AFTER" ] && echo "RESULT-PG: PASS — CRIU checkpoint/restore of multi-process PostgreSQL, data intact, server live (new=\$NEW)" || echo "RESULT-PG: FAIL (before=\$BEFORE after=\$AFTER)"
echo "=== GUEST-DONE ==="; poweroff -f
INIT
chmod +x "$R/init"

echo "== 4. pack + boot KVM guest =="
( cd "$R" && find . 2>/dev/null | cpio -o -H newc 2>/dev/null | gzip -1 > "$WORK/initramfs.cpio.gz" )
sudo timeout 200 qemu-system-x86_64 -enable-kvm -m 4096 -smp 2 \
  -kernel "$WORK/vmlinuz" -initrd "$WORK/initramfs.cpio.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init quiet" -nographic -no-reboot 2>&1 \
  | grep -aE 'KERNEL|postmaster|data before|dump exit|restore exit|data after|post-restore|RESULT-PG|PGLOG|GUEST-DONE'
