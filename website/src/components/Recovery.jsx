import { useRef } from "react";
import { Section, PlayerHead, StageChips } from "./ui";
import { useLoop, seg, eseg, lerp, pulse } from "../lib/anim";

const W = 960, H = 480;
const PRIM = { cx: 330, cy: 150, x: 240, y: 108, w: 180, h: 84 };
const REPL = { cx: 800, cy: 130, x: 700, y: 94, w: 200, h: 72 };
const CLIENT = { x: 744, y: 300, w: 156, h: 60 };
const LOG = { x0: 120, y: 408, w: 44, gap: 9 };
const cellX = (i) => LOG.x0 + i * (LOG.w + LOG.gap);

const N_LIVE = 9;                       // ts 1..9 before the crash
const dep = (i) => 0.03 + i * 0.042;    // input i leaves the fabric
const arr = (i) => dep(i) + 0.028;      // reaches the primary
const ackT = (i) => arr(i) + 0.045;     // client ack

const SNAP_T = 0.44, SNAP_CELL = 5;     // checkpoint after ts6
const CRASH_A = 0.50, CRASH_B = 0.56;
const REST_A = 0.56, REST_B = 0.64;
const replayT = (j) => 0.66 + j * 0.055; // replay ts7..9
const RESUME_DEP = 0.86;

const STAGES = [
  { at: 0.0, label: "live" },
  { at: SNAP_T - 0.02, label: "checkpoint @ T" },
  { at: CRASH_A, label: "crash" },
  { at: REST_A, label: "restore" },
  { at: 0.66, label: "replay + suppress" },
  { at: 0.84, label: "resume · exactly-once" },
];

const CAPTIONS = [
  <>
    <b>① Live.</b> The fabric delivers inputs in <b>ts order</b>. Each input is appended to
    the durable log, <b>scattered to the in-fabric replica at its total-order position</b>{" "}
    (1 RTT), applied, and its reply released once the commit barrier passes — the per-client
    high-water-mark (<span className="mono">hwm</span>) tracks the last acknowledged sequence.
  </>,
  <>
    <b>② Checkpoint.</b> When the barrier passes a snapshot timestamp <i>T</i>, every node
    independently quiesces inputs &gt; <i>T</i>, drains ≤ <i>T</i>, and checkpoints —
    state <b>and</b> the hwm map. No markers, no channel state: the cut is empty by
    construction.
  </>,
  <>
    <b>③ Crash.</b> <span className="mono">kill -9</span>. All volatile state is gone. But the
    log prefix is already replicated — every acknowledged input is durable on a survivor.
  </>,
  <>
    <b>④ Restore.</b> A replacement loads the latest snapshot (state + hwm), bounding replay
    to the post-checkpoint tail. Recovery is affine in the tail: ~30 ms floor + ~0.5 ms per
    1,000 requests.
  </>,
  <>
    <b>⑤ Replay.</b> The suffix re-runs in <b>ts order — no order-log was ever written</b>.
    The libOS replays the same virtual-clock deltas and RNG stream, so re-execution is
    byte-identical. Re-emitted outputs with seq ≤ hwm are <b>suppressed</b>: each acknowledged
    effect externalizes exactly once.
  </>,
  <>
    <b>⑥ Resume.</b> Past the recovered cut, delivery goes live again. Verified under injected
    crashes: 191,073 acked writes, 0 lost, 0 torn — and a Wing–Gong checker certifies the
    contended history <b>linearizable</b>, including the post-recovery read.
  </>,
];

export default function Recovery() {
  const ref = useRef(null);
  const { t, playing, setPlaying, seek } = useLoop(26000, ref);
  const stageIdx = STAGES.findIndex(
    (s, i) => t >= s.at && (i === STAGES.length - 1 || t < STAGES[i + 1].at)
  );

  const dead = t >= CRASH_A && t < REST_A;
  const restoring = t >= REST_A && t < REST_B;
  const acked = Array.from({ length: N_LIVE }, (_, i) => t >= ackT(i)).filter(Boolean).length;
  const resumeAcked = t >= RESUME_DEP + 0.075;
  const hwm = t < CRASH_A ? acked : resumeAcked ? 10 : 9;
  const scattered = Array.from({ length: N_LIVE }, (_, i) => t >= arr(i) + 0.02).filter(Boolean).length;

  return (
    <Section
      id="recovery"
      ts="005"
      kicker="§5 · Crash, replay, exactly-once"
      title={
        <>
          Kill it. Wait. Recover it.
          Demand the bytes match.
        </>
      }
    >
      <p className="lede">
        The recovered replica is a deterministic function of the fabric-ordered input prefix up
        to the last durable barrier — so “the producing state is durable” reduces to “the input
        prefix is replicated,” which the fabric already does. Here is the whole life cycle.
      </p>

      <div className="panel" style={{ marginTop: 34 }} ref={ref}>
        <PlayerHead
          title="anim 03 · the OneBarrier engine — live · crash · replay"
          playing={playing}
          setPlaying={setPlaying}
        />
        <StageChips stages={STAGES} t={t} seek={seek} />
        <div style={{ padding: "6px 10px 0" }}>
          <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage" role="img"
            aria-label="Animation of OneBarrier recovery: live inputs are logged and scattered to a replica; the primary crashes; a replacement restores the snapshot, replays the log suffix in timestamp order, suppresses already-acknowledged outputs, and resumes live delivery exactly once.">

            {/* fabric input rail */}
            <line x1="20" y1={PRIM.cy} x2={PRIM.x - 4} y2={PRIM.cy} stroke="var(--hairline)" strokeWidth="1" />
            <text x="22" y={PRIM.cy - 14} fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)">
              fabric delivers in ts order
            </text>

            {/* primary */}
            <g opacity={dead ? 0.55 : 1}>
              <rect x={PRIM.x} y={PRIM.y} width={PRIM.w} height={PRIM.h} rx="9"
                fill={dead ? "var(--red-dim)" : restoring ? "var(--amber-dim)" : "var(--surface-2)"}
                stroke={dead ? "var(--red)" : restoring ? "var(--amber)" : "var(--hairline)"}
                strokeWidth="1.3" strokeDasharray={restoring ? "6 4" : "none"} />
              <text x={PRIM.cx} y={PRIM.y + 26} fill="var(--ink)" fontSize="12.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                {t >= REST_A ? "replacement replica" : "primary (unmodified + libOS)"}
              </text>
              <text x={PRIM.cx} y={PRIM.y + 46} fill="var(--ink-2)" fontSize="10.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                deterministic state machine
              </text>
              <text x={PRIM.cx} y={PRIM.y + 66} fill="var(--amber)" fontSize="11"
                fontFamily="var(--font-mono)" textAnchor="middle">
                hwm = {hwm}
              </text>
            </g>
            {dead && (
              <g>
                <text x={PRIM.cx} y={PRIM.cy - 54} fill="var(--red)" fontSize="15"
                  fontFamily="var(--font-mono)" textAnchor="middle" fontWeight="600">
                  ✕ kill -9
                </text>
                <circle cx={PRIM.cx} cy={PRIM.cy} r={40 + pulse(t, CRASH_A, CRASH_B) * 30}
                  fill="none" stroke="var(--red)" strokeWidth="1.4" opacity={pulse(t, CRASH_A, CRASH_B)} />
              </g>
            )}
            {restoring && (
              <text x={PRIM.cx} y={PRIM.cy - 54} fill="var(--amber)" fontSize="11.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                ← restore snapshot @ T (state + hwm)
              </text>
            )}

            {/* replica */}
            <rect x={REPL.x} y={REPL.y} width={REPL.w} height={REPL.h} rx="9"
              fill="var(--surface-2)" stroke="rgba(25,158,112,0.45)" strokeWidth="1.2" />
            <text x={REPL.cx} y={REPL.y + 22} fill="var(--green)" fontSize="12"
              fontFamily="var(--font-mono)" textAnchor="middle">in-fabric replica</text>
            <text x={REPL.cx} y={REPL.y + 40} fill="var(--ink-2)" fontSize="10.5"
              fontFamily="var(--font-mono)" textAnchor="middle">durable log copy · survives</text>
            {/* mini log mirror */}
            {Array.from({ length: 10 }, (_, i) => (
              <rect key={i} x={REPL.x + 14 + i * 18} y={REPL.y + 50} width="14" height="10" rx="2"
                fill={i < scattered || (i === 9 && t >= RESUME_DEP + 0.05) ? "var(--green-dim)" : "transparent"}
                stroke={i < scattered || (i === 9 && t >= RESUME_DEP + 0.05) ? "var(--green)" : "var(--hairline)"}
                strokeWidth="1" />
            ))}

            {/* client */}
            <rect x={CLIENT.x} y={CLIENT.y} width={CLIENT.w} height={CLIENT.h} rx="9"
              fill="var(--surface-2)" stroke="var(--hairline)" />
            <text x={CLIENT.x + CLIENT.w / 2} y={CLIENT.y + 24} fill="var(--ink)" fontSize="12"
              fontFamily="var(--font-mono)" textAnchor="middle">client</text>
            <text x={CLIENT.x + CLIENT.w / 2} y={CLIENT.y + 44} fill="var(--ink-2)" fontSize="10.5"
              fontFamily="var(--font-mono)" textAnchor="middle">
              last acked seq = {t < CRASH_A ? acked : resumeAcked ? 10 : 9}
            </text>

            {/* durable log tape */}
            <text x={LOG.x0} y={LOG.y - 30} fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)">
              durable ordered log — (ts, op), no order-log needed
            </text>
            {Array.from({ length: 10 }, (_, i) => {
              const filled = i < N_LIVE ? t >= arr(i) : t >= RESUME_DEP + 0.03;
              const replayIdx = i - 6; // cells 6,7,8 replay as ts7..9
              const replaying =
                replayIdx >= 0 && replayIdx < 3 &&
                t >= replayT(replayIdx) && t < replayT(replayIdx) + 0.05;
              const replayed = replayIdx >= 0 && replayIdx < 3 && t >= replayT(replayIdx) && t < 0.995;
              const inSnap = i <= SNAP_CELL && t >= SNAP_T;
              return (
                <g key={i}>
                  <rect x={cellX(i)} y={LOG.y - 18} width={LOG.w} height="30" rx="4"
                    fill={replaying ? "var(--green-dim)" : filled ? (inSnap ? "var(--amber-dim)" : "var(--blue-dim)") : "transparent"}
                    stroke={replaying || (replayed && t >= 0.66) ? "var(--green)" : filled ? (inSnap ? "rgba(242,177,61,0.5)" : "var(--blue)") : "var(--hairline)"}
                    strokeWidth={replaying ? 1.8 : 1} />
                  {filled && (
                    <text x={cellX(i) + LOG.w / 2} y={LOG.y + 2} fill="var(--ink)" fontSize="11"
                      fontFamily="var(--font-mono)" textAnchor="middle">{i + 1}</text>
                  )}
                </g>
              );
            })}
            {/* snapshot marker */}
            {t >= SNAP_T && (
              <g>
                <line x1={cellX(SNAP_CELL) + LOG.w + LOG.gap / 2} y1={LOG.y - 34}
                  x2={cellX(SNAP_CELL) + LOG.w + LOG.gap / 2} y2={LOG.y + 22}
                  stroke="var(--amber)" strokeWidth="1.6" />
                <text x={cellX(SNAP_CELL) + LOG.w + LOG.gap / 2} y={LOG.y + 40}
                  fill="var(--amber)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
                  snapshot @ T=6
                </text>
              </g>
            )}
            {t >= SNAP_T && t < SNAP_T + 0.05 && (
              <circle cx={PRIM.cx} cy={PRIM.cy} r={36 + pulse(t, SNAP_T, SNAP_T + 0.05) * 22}
                fill="none" stroke="var(--amber)" strokeWidth="1.4"
                opacity={pulse(t, SNAP_T, SNAP_T + 0.05)} />
            )}
            {/* replay arrow from log to primary */}
            {t >= 0.66 && t < 0.86 && (
              <g>
                <path d={`M ${cellX(7)} ${LOG.y - 24} C ${cellX(7) - 30} ${LOG.y - 90}, ${PRIM.cx + 60} ${PRIM.cy + 90}, ${PRIM.cx + 10} ${PRIM.cy + 46}`}
                  fill="none" stroke="var(--green)" strokeWidth="1.3" strokeDasharray="5 4" />
                <text x={cellX(7) + 6} y={LOG.y - 66} fill="var(--green)" fontSize="10.5" fontFamily="var(--font-mono)">
                  replay suffix in ts order
                </text>
              </g>
            )}

            {/* live input dots + scatter + acks */}
            {Array.from({ length: N_LIVE }, (_, i) => {
              const u = seg(t, dep(i), arr(i));
              if (t < dep(i) || t > arr(i) + 0.001) return null;
              return (
                <circle key={i} cx={lerp(24, PRIM.x - 6, eseg(t, dep(i), arr(i)))} cy={PRIM.cy}
                  r="10" fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.3" />
              );
            })}
            {/* scatter dots to replica */}
            {Array.from({ length: N_LIVE }, (_, i) => {
              const s0 = arr(i), s1 = arr(i) + 0.02;
              if (t < s0 || t > s1) return null;
              const u = eseg(t, s0, s1);
              return (
                <circle key={i} cx={lerp(PRIM.x + PRIM.w, REPL.x, u)}
                  cy={lerp(PRIM.cy - 20, REPL.cy, u) - 14 * Math.sin(Math.PI * u)}
                  r="6" fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.2" />
              );
            })}
            {/* ack envelopes to client */}
            {Array.from({ length: N_LIVE }, (_, i) => {
              const a0 = arr(i) + 0.012, a1 = ackT(i);
              if (t < a0 || t > a1) return null;
              const u = eseg(t, a0, a1);
              return (
                <rect key={i} x={lerp(PRIM.x + PRIM.w, CLIENT.x - 10, u) - 11}
                  y={lerp(PRIM.cy + 16, CLIENT.y + 18, u)} width="22" height="15" rx="3"
                  fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.1" />
              );
            })}

            {/* suppressed replay outputs */}
            {[0, 1, 2].map((j) => {
              const r0 = replayT(j), r1 = r0 + 0.045;
              if (t < r0 + 0.01 || t > r1 + 0.03) return null;
              const u = eseg(t, r0 + 0.01, r1);
              const fade = 1 - seg(t, r1, r1 + 0.03);
              return (
                <g key={j} opacity={fade}>
                  <rect x={PRIM.x + PRIM.w + 16 + u * 40 - 11} y={PRIM.cy + 16} width="22" height="15" rx="3"
                    fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.1" />
                  <text x={PRIM.x + PRIM.w + 66 + u * 40} y={PRIM.cy + 28} fill="var(--red)" fontSize="10.5"
                    fontFamily="var(--font-mono)">
                    seq {7 + j} ≤ hwm — suppressed
                  </text>
                </g>
              );
            })}

            {/* resume: ts10 */}
            {t >= RESUME_DEP && t < RESUME_DEP + 0.028 && (
              <circle cx={lerp(24, PRIM.x - 6, eseg(t, RESUME_DEP, RESUME_DEP + 0.028))} cy={PRIM.cy}
                r="10" fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.3" />
            )}
            {t >= RESUME_DEP + 0.03 && t < RESUME_DEP + 0.075 && (
              <rect x={lerp(PRIM.x + PRIM.w, CLIENT.x - 10, eseg(t, RESUME_DEP + 0.03, RESUME_DEP + 0.075)) - 11}
                y={lerp(PRIM.cy + 16, CLIENT.y + 18, eseg(t, RESUME_DEP + 0.03, RESUME_DEP + 0.075))}
                width="22" height="15" rx="3" fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.1" />
            )}
            {resumeAcked && (
              <g>
                <rect x={CLIENT.x + 8} y={CLIENT.y - 34} width="140" height="24" rx="12"
                  fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.1" />
                <text x={CLIENT.x + 78} y={CLIENT.y - 18} fill="var(--green)" fontSize="11"
                  fontFamily="var(--font-mono)" textAnchor="middle">
                  exactly-once ✓
                </text>
              </g>
            )}
          </svg>
        </div>
        <div className="panel-caption" style={{ minHeight: 96 }}>
          {CAPTIONS[Math.max(stageIdx, 0)]}
        </div>
      </div>

      <div className="note-amber">
        <b>Machine-checked.</b>&nbsp; The exactly-once and no-lost-acknowledged-write invariants
        are verified in TLA+ under arbitrary crash/recover interleavings, and 1Pipe’s total
        order over 3.5×10⁶ explored states — protocol abstractions checked by TLC, the Rust/C
        implementations validated separately by crash injection and a linearizability checker.
      </div>
    </Section>
  );
}
