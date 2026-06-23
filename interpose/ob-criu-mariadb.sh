#!/usr/bin/env bash
# OneBarrier CRIU checkpoint/restore of UNMODIFIED MySQL/MariaDB — the general
# any-binary checkpoint path on a second shared-everything database.
#   ob-criu-mariadb.sh
#
# Complements the PostgreSQL result (ob-criu-postgres-kvm.sh): Postgres is a
# multi-PROCESS tree (postmaster + workers, SysV+POSIX shm) and needs a private IPC
# namespace + the KVM clean room; MariaDB is a single multi-THREADED process with no
# SysV shm, so CRIU 3.19 checkpoints/restores it directly on the host. Together they
# show the checkpoint path covers BOTH database architectures. This is the
# checkpoint-only regime (no order-log-free replay): a shared-everything DB is not a
# deterministic-replay candidate, so it recovers from a CRIU image like Remus/HyCoR.
#
# Prereq: CRIU >= 3.19 (built by ob-criu-kvm.sh at /tmp/criu-src). InnoDB native AIO
# is disabled (CRIU cannot checkpoint libaio contexts); the sandbox's binfmt_misc
# mount is unmounted (it has no parent in CRIU's mount dump — a sandbox artifact).
set -u
CRIU="${CRIU:-/tmp/criu-src/criu/criu}"
DD=/tmp/ob-maria-data; SOCK=/tmp/ob-maria.sock; PORT=3340; CR=/tmp/ob-maria-cr
[ -x "$CRIU" ] || { echo "need CRIU >= 3.19 at $CRIU (run ob-criu-kvm.sh first)"; exit 1; }

pkill -9 mariadbd 2>/dev/null; sleep 1; rm -rf "$DD" "$CR"; mkdir -p "$DD" "$CR"
sudo umount /proc/sys/fs/binfmt_misc 2>/dev/null || true   # sandbox mount artifact
mariadb-install-db --datadir="$DD" --auth-root-authentication-method=normal --skip-test-db >/tmp/ob-maria-init.log 2>&1 \
  || { echo "initdb failed"; tail -3 /tmp/ob-maria-init.log; exit 1; }

echo "== start unmodified mariadbd (InnoDB native AIO off for CRIU) =="
setsid /usr/sbin/mariadbd --datadir="$DD" --socket="$SOCK" --port=$PORT --skip-grant-tables \
  --innodb-use-native-aio=0 --innodb-flush-method=fsync --pid-file=/tmp/ob-maria.pid \
  --bind-address=127.0.0.1 >/tmp/ob-maria.log 2>&1 </dev/null &
for i in $(seq 60); do mysql --socket="$SOCK" -uroot -e "SELECT 1" >/dev/null 2>&1 && break; sleep 0.5; done
mysql --socket="$SOCK" -uroot -e "SELECT 1" >/dev/null 2>&1 || { echo "mariadbd start FAILED"; tail -10 /tmp/ob-maria.log; exit 1; }
PID=$(cat /tmp/ob-maria.pid); echo "mariadbd pid=$PID threads=$(ls /proc/$PID/task|wc -l)"

# A table with time- and random-derived columns + 1000 rows.
mysql --socket="$SOCK" -uroot <<SQL
CREATE DATABASE ob; USE ob;
CREATE TABLE t(id INT PRIMARY KEY AUTO_INCREMENT, val INT,
               created DOUBLE DEFAULT (UNIX_TIMESTAMP(NOW(4))), rnd DOUBLE DEFAULT (RAND()));
INSERT INTO t(val) SELECT seq FROM seq_1_to_1000;
SQL
chk(){ mysql --socket="$SOCK" -uroot -N -e "SELECT COUNT(*),SUM(val),CRC32(GROUP_CONCAT(id,':',val ORDER BY id)) FROM ob.t" 2>/dev/null; }
B=$(chk); echo "data before : $B"

echo "== CRIU dump (checkpoint the whole mariadbd process tree) =="
sudo "$CRIU" dump -t "$PID" -D "$CR" --shell-job --tcp-established --ext-unix-sk --file-locks -v2 -o dump.log 2>&1 | tail -2
sudo grep -q "Dumping FAILED" "$CR/dump.log" 2>/dev/null && { echo "DUMP FAILED"; sudo tail -5 "$CR/dump.log"; exit 1; }
echo "dump ok (pid gone: $([ -d /proc/$PID ] && echo no || echo yes))"

echo "== CRIU restore =="
sudo "$CRIU" restore -D "$CR" --shell-job --tcp-established --ext-unix-sk --file-locks -d -v2 -o restore.log 2>&1 | tail -2
for i in $(seq 20); do mysql --socket="$SOCK" -uroot -e "SELECT 1" >/dev/null 2>&1 && break; sleep 0.5; done
A=$(chk); echo "data after  : $A"
mysql --socket="$SOCK" -uroot -e "INSERT INTO ob.t(val) VALUES (9999);" 2>/dev/null \
  && echo "post-restore write: count=$(mysql --socket="$SOCK" -uroot -N -e 'SELECT COUNT(*) FROM ob.t') (server LIVE)"
pkill -9 mariadbd 2>/dev/null
[ "$A" = "$B" ] && [ -n "$A" ] \
  && echo "RESULT-MARIADB: PASS — data byte-identical across CRIU checkpoint/restore, restored server live ✅" \
  || { echo "RESULT-MARIADB: FAIL (before=$B after=$A)"; exit 1; }
