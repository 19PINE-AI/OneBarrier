---
name: Determinism failure
about: An app does not recover byte-identically
title: '[determinism] '
labels: determinism
---

## The app

Exact launch command, including every flag:

```
```

## What diverged

The probe values from all three runs. The control matters: if it matches live,
the test could have passed trivially and the failure means something different.

```
live   :
replay :
control:
```

Which output field differs — a timestamp, an ID, an eviction set, a counter?

## Environment

```
# paste the output of:
onebarrier doctor
uname -a
```

## Already checked

The known residual sources are listed in
[docs/your-app.md](../../docs/your-app.md#step-4--hunt-the-residual-nondeterminism).
Which have you ruled out?

- [ ] Timer-driven maintenance threads (memcached-style)
- [ ] `/dev/urandom` via `fopen` (bypasses both symbol interposition and the syscall trap)
- [ ] ASLR / RDRAND (needs `setarch -R` and `OPENSSL_ia32cap`)
- [ ] Fork-per-request resetting the per-process tick counter
- [ ] Single-threaded configuration confirmed
