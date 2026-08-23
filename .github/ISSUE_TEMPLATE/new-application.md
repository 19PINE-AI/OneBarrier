---
name: New application harness
about: You got a program recovering byte-identically
title: '[app] '
labels: enhancement, application
---

Thanks, this is the most useful thing to add to this project.

## App

Name, version, launch command:

```
```

## Probe

What time- or randomness-dependent output proves recovery worked? redis uses `TIME`,
nginx the `Date:` header, SQLite a `strftime('now')` column.

## Verdict

Both halves, `replay == live` and `control != live`:

```
live   :
replay :
control:
```

## What it needed

Any flags, config, or workarounds: a single-thread flag, disabled maintenance threads,
an explicit date header. These belong in the docs.

## Harness

Did you write an `interpose/ob-<app>.sh`? A PR is very welcome, see
[CONTRIBUTING.md](../../CONTRIBUTING.md).
