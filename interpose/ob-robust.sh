#!/usr/bin/env bash
# ob-robust.sh <app> <trials> — determinism ROBUSTNESS: run the record/crash/replay
# determinism cycle N independent times and report how many trials recover
# byte-identically (recovered == live) with the control diverging. Upgrades the
# single-run byte-identical result to a statistic: flaky nondeterminism (an
# occasional RDTSC/RDRAND/scheduling leak) would show up as a trial where
# recovered != live.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="${1:-redis}"
N="${2:-25}"
pass=0; fail=0
echo "ob-robust: $N trials of '$APP' record->crash->replay determinism"
for t in $(seq 1 "$N"); do
  out="$(bash "$HERE/ob-state-recovery.sh" "$APP" 2>/dev/null)"
  # one RESULT line per app demo; require ALL of them to be the success (✅) line
  res="$(printf '%s\n' "$out" | grep -c 'RESULT: .*✅')"
  tot="$(printf '%s\n' "$out" | grep -c 'RESULT:')"
  if [ "$res" -ge 1 ] && [ "$res" -eq "$tot" ]; then
    pass=$((pass+1)); printf '.'
  else
    fail=$((fail+1)); printf 'X'
    printf '%s\n' "$out" | grep 'RESULT:'
  fi
done
echo
echo "ob-robust[$APP]: $pass/$N trials recovered byte-identically (control diverged); $fail failed."
[ "$fail" -eq 0 ] && echo "ROBUST: determinism held on every independent trial." || echo "FLAKY: $fail trial(s) showed residual nondeterminism."
