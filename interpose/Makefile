# OneBarrier libOS shims.
#
# Three independent LD_PRELOAD libraries. Each closes one source of
# nondeterminism and each is useful on its own:
#
#   libobpreload.so  time      — virtual clock + socket capture
#   librngdet.so     randomness — seccomp trap on the raw getrandom(2) syscall
#   libdetsched.so   threads   — Kendo-style deterministic lock ordering
#
# Commodity hardware only: no RDMA, no kernel module, no root.

CC      ?= gcc
CFLAGS  ?= -O2 -Wall
LDFLAGS ?= -ldl -lpthread

SHIMS := libobpreload.so librngdet.so libdetsched.so

.PHONY: all clean
all: $(SHIMS)

%.so: %.c
	$(CC) -shared -fPIC $(CFLAGS) -o $@ $< $(LDFLAGS)

# -Wno-nonnull-compare: obpreload defensively NULL-checks arguments that the
# libc prototypes declare nonnull. Real callers do pass NULL, and an interposer
# that segfaults on them would take the application down with it.
libobpreload.so: obpreload.c
	$(CC) -shared -fPIC $(CFLAGS) -Wno-nonnull-compare -o $@ $< $(LDFLAGS)

librngdet.so: rngdet.c
	$(CC) -shared -fPIC $(CFLAGS) -o $@ $< $(LDFLAGS)

libdetsched.so: detsched.c
	$(CC) -shared -fPIC $(CFLAGS) -o $@ $< $(LDFLAGS)

clean:
	rm -f $(SHIMS) rdma_ftbench clicknf/ob_clicknf
