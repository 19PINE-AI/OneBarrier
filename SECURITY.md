# Security

## Reporting

Please report security issues privately rather than as a public issue, either through
GitHub's [private vulnerability reporting](https://github.com/19PINE-AI/OneBarrier/security/advisories/new)
or by email to `boj@19pine.ai`. I'll acknowledge within a few days. This is a research
project maintained by one person, so please be patient with timelines.

## Scope

OneBarrier is a research prototype. It hasn't been audited, and its threat model is
fail-stop crashes, not a malicious actor. Nodes are assumed to fail by stopping.
Byzantine faults are out of scope by design.

Some sharp edges worth knowing about if you plan to run it:

**The determinism stack deliberately destroys entropy.** `librngdet.so` replaces
`getrandom(2)` with a deterministic stream and the stack disables RDRAND. Assume any
process running under it has predictable randomness. Don't generate keys, session
tokens, or nonces in such a process and treat them as secret. This isn't a flaw,
determinism is the point, but it's easy to forget.

**Captures contain request bytes.** `capture.log` holds the raw bytes of every
intercepted request, which means credentials and personal data if your workload carries
them. Sessions live in `~/.onebarrier/` with normal user permissions. Treat them as
sensitive and scrub before attaching one to a bug report.

**`setarch -R` disables ASLR** for the process it launches, which removes a standard
exploitation mitigation. It's needed for byte-identical V8 randomness and you can skip
it with `--no-rng`.

Reports that go beyond what's documented here are welcome, particularly a way to escape
the intended scope of the interposition, or a case where a capture gets written
somewhere it shouldn't.
