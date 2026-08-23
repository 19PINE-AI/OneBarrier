---
name: Determinism failure
about: An app doesn't recover byte-identically
title: '[determinism] '
labels: determinism
---

## App

Exact launch command, flags included:

```
```

## What diverged

All three probe values. The control matters: if it matches live, the test could have
passed trivially and the failure means something different.

```
live   :
replay :
control:
```

Which field differs? A timestamp, an ID, an eviction set, a counter?

## Environment

```
# output of:
onebarrier doctor
uname -a
```

## Already ruled out

Known leftovers are listed in
[docs/your-app.md](../../docs/your-app.md#4-chase-the-leftovers).

- [ ] timer-driven maintenance threads (the memcached case)
- [ ] `/dev/urandom` via `fopen`, which misses both the symbol hook and the syscall trap
- [ ] ASLR / RDRAND (needs `setarch -R` and `OPENSSL_ia32cap`)
- [ ] fork-per-request resetting the tick counter
- [ ] single-threaded config confirmed
