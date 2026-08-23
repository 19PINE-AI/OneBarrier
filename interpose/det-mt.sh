#!/usr/bin/env bash
# OneBarrier deterministic multithreading (DMT) demonstration.
#   det-mt.sh
# Shows that libdetsched.so makes a multithreaded program's critical-section
# interleaving DETERMINISTIC (identical across runs) where it is otherwise
# scheduler-dependent, without deadlocking on condvars or real servers.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=ob-common.sh
. "$HERE/ob-common.sh"
ob_require_shims libdetsched.so || exit 1
SO="$HERE/libdetsched.so"

# --- microbenchmark: 4 threads append their id to a shared log under a mutex ---
cat > /tmp/dmt.c <<'C'
#include <stdio.h>
#include <pthread.h>
#define NT 4
#define ITERS 2000
static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static int lg[NT*ITERS]; static int n=0;
static void *worker(void *a){ long id=(long)a;
  for(int i=0;i<ITERS;i++){ pthread_mutex_lock(&m); lg[n++]=id; pthread_mutex_unlock(&m);} return 0; }
int main(){ pthread_t t[NT];
  for(long i=0;i<NT;i++) pthread_create(&t[i],0,worker,(void*)i);
  for(int i=0;i<NT;i++) pthread_join(t[i],0);
  unsigned long h=1469598103934665603UL;
  for(int i=0;i<n;i++){ h^=(unsigned char)lg[i]; h*=1099511628211UL; }
  printf("order_hash=%016lx n=%d\n", h, n); return 0; }
C
gcc -O2 -o /tmp/dmt /tmp/dmt.c -lpthread

echo "=== 1. multithreaded order determinism ==="
echo "WITHOUT detsched (OS scheduler picks the interleaving → hash varies):"
for i in 1 2 3; do /tmp/dmt; done
echo "WITH detsched (deterministic logical clocks → hash IDENTICAL, 0 relaxations):"
for i in 1 2 3; do OB_DETSCHED_STATS=1 OB_DETSCHED=1 LD_PRELOAD="$SO" /tmp/dmt; done

echo
echo "=== 2. condvar producer/consumer (must not deadlock under DMT) ==="
cat > /tmp/dpc.c <<'C'
#include <stdio.h>
#include <pthread.h>
#define NP 3
#define NC 3
#define ITEMS 1000
static pthread_mutex_t m=PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cv=PTHREAD_COND_INITIALIZER;
static int buf=0, produced=0, consumed=0;
static void *prod(void*a){ for(int i=0;i<ITEMS;i++){ pthread_mutex_lock(&m);
   while(buf>=8) pthread_cond_wait(&cv,&m); buf++; produced++; pthread_cond_signal(&cv); pthread_mutex_unlock(&m);} return 0;}
static void *cons(void*a){ while(1){ pthread_mutex_lock(&m);
   while(buf==0 && produced<NP*ITEMS) pthread_cond_wait(&cv,&m);
   if(buf==0 && produced>=NP*ITEMS){ pthread_mutex_unlock(&m); break;}
   buf--; consumed++; pthread_cond_signal(&cv); pthread_mutex_unlock(&m);} return 0;}
int main(){ pthread_t p[NP],c[NC];
  for(long i=0;i<NP;i++) pthread_create(&p[i],0,prod,0);
  for(long i=0;i<NC;i++) pthread_create(&c[i],0,cons,0);
  for(int i=0;i<NP;i++) pthread_join(p[i],0);
  pthread_mutex_lock(&m); pthread_cond_broadcast(&cv); pthread_mutex_unlock(&m);
  for(int i=0;i<NC;i++) pthread_join(c[i],0);
  printf("produced=%d consumed=%d\n",produced,consumed); return 0;}
C
gcc -O2 -o /tmp/dpc /tmp/dpc.c -lpthread
timeout 30 env OB_DETSCHED=1 LD_PRELOAD="$SO" /tmp/dpc && echo "no deadlock ✅" || echo "DEADLOCK ✗"

echo
echo "=== 3. composes with a real multithreaded server (memcached -t 4) ==="
if command -v memcached >/dev/null; then
  P=11270
  for pid in $(ss -tlnp 2>/dev/null|grep ":$P "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done
  OB_DETSCHED=1 LD_PRELOAD="$SO" memcached -p $P -U 0 -t 4 -m 64 >/tmp/dmt-mc.log 2>&1 &
  sleep 3
  r=$(printf 'set k 0 0 3\r\nabc\r\nget k\r\nquit\r\n'|timeout 2 nc 127.0.0.1 $P 2>/dev/null|tr -d '\r'|grep -c abc)
  [ "${r:-0}" -ge 1 ] && echo "memcached -t 4 (4 worker threads) serves+stores under DMT ✅" || echo "memcached did not serve ✗"
  for pid in $(ss -tlnp 2>/dev/null|grep ":$P "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done
else
  echo "memcached not installed — skipping"
fi
