---
name: New application harness
about: You got a program recovering byte-identically
title: '[app] '
labels: enhancement, application
---

Thank you — this is the most useful contribution to this project.

## The application

Name, version, and launch command:

```
```

## The probe

What time- or randomness-dependent output proves recovery worked? (Redis uses
`TIME`, nginx the `Date:` header, SQLite a `strftime('now')` column.)

## The verdict

Both halves are required — `replay == live` **and** `control != live`:

```
live   :
replay :
control:
```

## What it needed

Any flags, configuration, or workarounds — a single-thread flag, disabled
maintenance threads, an explicit date header. These belong in the docs.

## Harness

Have you written an `interpose/ob-<app>.sh`? A PR is very welcome — see
[CONTRIBUTING.md](../../CONTRIBUTING.md).
