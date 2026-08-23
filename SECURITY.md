# Security policy

## Reporting a vulnerability

Please report security issues privately, not as a public issue:

- GitHub's [private vulnerability reporting](https://github.com/19PINE-AI/OneBarrier/security/advisories/new), or
- email the maintainer at `boj@19pine.ai`

Expect an acknowledgement within a few days. This is a research project
maintained by one person; please be patient with the timeline.

## Scope, and what to expect

OneBarrier is a research prototype. It has not undergone a security audit, and
its threat model is **fail-stop crashes** — not a malicious actor. It assumes
nodes fail by stopping, not by behaving adversarially. Byzantine faults are out
of scope by design.

That said, some properties of the design are worth stating plainly, because they
matter to anyone considering running it:

**The shims are powerful by construction.** `LD_PRELOAD` interposes on libc,
`librngdet.so` installs a seccomp filter with a user-notification supervisor, and
the RNG stack deliberately weakens entropy: it replaces `getrandom(2)` with a
deterministic stream and disables RDRAND. **A program running under the
determinism stack should be assumed to have predictable randomness.** Do not
generate cryptographic keys, session tokens, or nonces in a process running under
`librngdet.so` and treat them as secret. This is not a flaw — determinism is the
entire point — but it is a sharp edge.

**Captures contain request data.** `capture.log` holds the raw bytes of every
intercepted request, which means credentials, tokens, and personal data if your
workload carries them. Session directories default to `~/.onebarrier/` with
ordinary user permissions. Treat them as sensitive, and do not attach one to a
bug report without scrubbing it.

**`setarch -R` disables ASLR** for the process it launches, removing a standard
exploitation mitigation. It is required for byte-identical V8 randomness, and it
is opt-out via `--no-rng`.

Reports about any of these that go beyond what is documented here are welcome —
particularly a way to escape the intended scope of the interposition, or a case
where a capture is written somewhere it should not be.
