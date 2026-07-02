import { useRef } from "react";
import { Section } from "./ui";
import { PlayerHead, StageChips } from "./ui";
import { useLoop, seg, eseg, lerp, pulse } from "../lib/anim";

/* ---------------- geometry ---------------- */
const W = 960, H = 480;
const SENDERS = [
  { name: "host A", x: 96, y: 92 },
  { name: "host B", x: 96, y: 206 },
  { name: "host C", x: 96, y: 320 },
];
const SWITCH = { x: 430, y: 206 };
const GATE = { x: 742, yTop: 118, slotH: 30 };
const PIPE = { y: 420, x0: 400, slotW: 62, xStart: 428 };

/* ---------------- message schedule ----------------
   arrival order at the gate: 5, 2, 7, 3, 11, 9  (≠ timestamp order) */
const MSGS = [
  { ts: 2,  host: 0, dep: 0.02, f1: 0.14, f2: 0.11, rank: 1 },
  { ts: 5,  host: 1, dep: 0.03, f1: 0.10, f2: 0.10, rank: 0 },
  { ts: 3,  host: 2, dep: 0.06, f1: 0.16, f2: 0.11, rank: 3 },
  { ts: 7,  host: 2, dep: 0.09, f1: 0.11, f2: 0.10, rank: 2 },
  { ts: 9,  host: 0, dep: 0.12, f1: 0.15, f2: 0.12, rank: 5 },
  { ts: 11, host: 1, dep: 0.15, f1: 0.11, f2: 0.10, rank: 4 },
];
const ORDER = [2, 3, 5, 7, 9, 11];
/* release times: the FIFO gate lets ts out once the barrier passes it */
const REL = { 2: 0.54, 3: 0.585, 5: 0.63, 7: 0.675, 9: 0.72, 11: 0.765 };
const REL_DUR = 0.05;
const COMMIT_A = 0.82, COMMIT_B = 0.93;

const STAGES = [
  { at: 0.0,  label: "stamp" },
  { at: 0.17, label: "aggregate" },
  { at: 0.36, label: "hold at gate" },
  { at: 0.52, label: "deliver in ts order" },
  { at: 0.80, label: "commit (reliable)" },
];

const CAPTIONS = [
  <>
    <b>① Stamp.</b> Each sender stamps outgoing messages with a timestamp from its
    loosely-synchronized local clock. No coordination yet — hosts A, B, C emit concurrently.
  </>,
  <>
    <b>② Aggregate.</b> As messages flow through the programmable switch, it aggregates a{" "}
    <b>barrier timestamp</b> — the minimum timestamp still in flight anywhere. Everything
    below the barrier is known to have arrived. The order is established <i>in the network</i>.
  </>,
  <>
    <b>③ Hold.</b> Messages reach the receiver <b>out of timestamp order</b> (here: 5, 2, 7,
    3, 11, 9) and wait in the FIFO gate. Nothing is delivered above the barrier.
  </>,
  <>
    <b>④ Deliver.</b> As the barrier advances past each timestamp, the gate releases it —
    so the application sees <b>one global timestamp order</b>, on every receiver, with no
    order-log written by anyone.
  </>,
  <>
    <b>⑤ Commit.</b> In <b>reliable</b> mode 1Pipe runs a two-phase commit: after end-to-end
    loss recovery, an aggregated <span style={{ color: "var(--amber)" }}>commit barrier</span>{" "}
    makes the group delivery atomic — ~1.5 RTT total. <b>Remember this barrier:</b> OneBarrier
    is built out of it.
  </>,
];

function msgPos(m, t) {
  const s = SENDERS[m.host];
  const t1 = m.dep + m.f1;           // arrive at switch
  const t2 = t1 + m.f2;              // arrive at gate slot
  const rel = REL[m.ts];             // release from gate
  const slotY = GATE.yTop + m.rank * GATE.slotH;
  const finalX = PIPE.xStart + ORDER.indexOf(m.ts) * PIPE.slotW;

  if (t < m.dep) return null;
  if (t < t1) {
    const u = eseg(t, m.dep, t1);
    return {
      x: lerp(s.x + 46, SWITCH.x - 4, u),
      y: lerp(s.y, SWITCH.y, u) - 16 * Math.sin(Math.PI * u),
      state: "flight",
    };
  }
  if (t < t2) {
    const u = eseg(t, t1, t2);
    return {
      x: lerp(SWITCH.x + 4, GATE.x, u),
      y: lerp(SWITCH.y, slotY, u) - 12 * Math.sin(Math.PI * u),
      state: "flight",
    };
  }
  if (t < rel) return { x: GATE.x, y: slotY, state: "held" };
  if (t < rel + REL_DUR) {
    const u = eseg(t, rel, rel + REL_DUR);
    return {
      x: lerp(GATE.x, finalX, u),
      y: lerp(slotY, PIPE.y, u) + 10 * Math.sin(Math.PI * u),
      state: "releasing",
    };
  }
  return { x: finalX, y: PIPE.y, state: "delivered" };
}

function barrierValue(t) {
  if (t < STAGES[3].at) return "…";
  let b = "…";
  for (const ts of ORDER) if (t >= REL[ts]) b = ts;
  return b;
}

export default function OnePipe() {
  const ref = useRef(null);
  const { t, playing, setPlaying, seek } = useLoop(20000, ref);
  const stageIdx = STAGES.findIndex(
    (s, i) => t >= s.at && (i === STAGES.length - 1 || t < STAGES[i + 1].at)
  );
  const commitX = lerp(PIPE.x0 - 20, W - 20, eseg(t, COMMIT_A, COMMIT_B));
  const commitOn = t >= COMMIT_A;

  return (
    <Section
      id="fabric"
      ts="002"
      kicker="§2 · The substrate: how 1Pipe works"
      title={
        <>
          One pipe: the network itself delivers every message
          in a single global timestamp order.
        </>
      }
    >
      <p className="lede">
        1Pipe provides <strong>scalable total-order communication</strong>: messages carry
        timestamps, an in-network barrier aggregation on a programmable switch establishes one
        global order, and a FIFO gate at each receiver delivers in timestamp order. Its{" "}
        <strong>reliable mode</strong> adds a two-phase commit — loss recovery, then an
        aggregated commit barrier that makes group delivery atomic — one extra round trip over
        best effort, at a 1–2&thinsp;µs RDMA operating point.
      </p>

      <div className="panel" style={{ marginTop: 34 }} ref={ref}>
        <PlayerHead
          title="anim 01 · the 1Pipe fabric, end to end"
          playing={playing}
          setPlaying={setPlaying}
        />
        <StageChips stages={STAGES} t={t} seek={seek} />
        <div style={{ padding: "6px 10px 0" }}>
          <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage" role="img"
            aria-label="Animation of the 1Pipe fabric: hosts stamp messages, a programmable switch aggregates a barrier timestamp, a FIFO gate delivers messages in timestamp order, and a commit barrier makes the group delivery atomic.">
            {/* links */}
            {SENDERS.map((s) => (
              <line key={s.name} x1={s.x + 46} y1={s.y} x2={SWITCH.x - 58} y2={SWITCH.y}
                stroke="var(--hairline)" strokeWidth="1" />
            ))}
            <line x1={SWITCH.x + 58} y1={SWITCH.y} x2={GATE.x - 40} y2={SWITCH.y}
              stroke="var(--hairline)" strokeWidth="1" />

            {/* senders */}
            {SENDERS.map((s, i) => (
              <g key={s.name}>
                <rect x={s.x - 46} y={s.y - 24} width="92" height="48" rx="7"
                  fill="var(--surface-2)" stroke="var(--hairline)" />
                <text x={s.x} y={s.y - 3} fill="var(--ink)" fontSize="12"
                  fontFamily="var(--font-mono)" textAnchor="middle">{s.name}</text>
                <text x={s.x} y={s.y + 14} fill="var(--muted)" fontSize="9.5"
                  fontFamily="var(--font-mono)" textAnchor="middle">local clock</text>
              </g>
            ))}
            <text x={96} y={40} fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
              senders stamp ts
            </text>

            {/* switch */}
            <g>
              <polygon
                points={`${SWITCH.x - 62},${SWITCH.y - 34} ${SWITCH.x + 62},${SWITCH.y - 34} ${SWITCH.x + 44},${SWITCH.y + 34} ${SWITCH.x - 44},${SWITCH.y + 34}`}
                fill="var(--surface-2)" stroke="var(--hairline)"
              />
              <text x={SWITCH.x} y={SWITCH.y - 8} fill="var(--ink)" fontSize="12"
                fontFamily="var(--font-mono)" textAnchor="middle">programmable</text>
              <text x={SWITCH.x} y={SWITCH.y + 8} fill="var(--ink)" fontSize="12"
                fontFamily="var(--font-mono)" textAnchor="middle">switch</text>
              <text x={SWITCH.x} y={SWITCH.y + 52} fill="var(--muted)" fontSize="10"
                fontFamily="var(--font-mono)" textAnchor="middle">in-network barrier aggregation</text>
              {/* barrier readout */}
              <rect x={SWITCH.x - 74} y={SWITCH.y + 62} width="148" height="26" rx="5"
                fill="var(--amber-dim)" stroke="rgba(242,177,61,0.4)" />
              <text x={SWITCH.x} y={SWITCH.y + 79} fill="var(--amber)" fontSize="11.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                barrier ts = {barrierValue(t)}
              </text>
            </g>

            {/* FIFO gate */}
            <g>
              <rect x={GATE.x - 40} y={GATE.yTop - 28} width="104" height={GATE.slotH * 6 + 40}
                rx="8" fill="var(--surface-2)" stroke="var(--hairline)" />
              <text x={GATE.x + 12} y={GATE.yTop - 40} fill="var(--ink)" fontSize="11.5"
                fontFamily="var(--font-mono)" textAnchor="middle">FIFO gate</text>
              <text x={GATE.x + 12} y={GATE.yTop + GATE.slotH * 6 + 26} fill="var(--muted)"
                fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="middle">
                hold until barrier ≥ ts
              </text>
              {/* amber gate edge */}
              <line x1={GATE.x + 64} y1={GATE.yTop - 28} x2={GATE.x + 64}
                y2={GATE.yTop + GATE.slotH * 6 + 12}
                stroke="var(--amber)" strokeWidth="1.6"
                opacity={stageIdx >= 2 ? 0.9 : 0.35} strokeDasharray="4 4" />
            </g>

            {/* delivered pipe */}
            <g>
              <rect x={PIPE.x0} y={PIPE.y - 22} width={W - PIPE.x0 - 16} height="44" rx="22"
                fill="none" stroke="var(--hairline)" strokeWidth="1.2" />
              <text x={PIPE.x0 + 4} y={PIPE.y - 34} fill="var(--muted)" fontSize="10.5"
                fontFamily="var(--font-mono)">
                delivered — one total order, every receiver
              </text>
            </g>

            {/* commit barrier sweep */}
            {commitOn && (
              <g>
                <line x1={commitX} y1={PIPE.y - 40} x2={commitX} y2={PIPE.y + 34}
                  stroke="var(--amber)" strokeWidth="2" />
                <line x1={commitX} y1={PIPE.y - 40} x2={commitX} y2={PIPE.y + 34}
                  stroke="var(--amber)" strokeWidth="8" opacity="0.22" />
                <text x={Math.min(commitX, W - 130)} y={PIPE.y + 52} fill="var(--amber)" fontSize="10.5"
                  fontFamily="var(--font-mono)" textAnchor="middle">
                  commit barrier · atomic
                </text>
              </g>
            )}

            {/* messages */}
            {MSGS.map((m) => {
              const p = msgPos(m, t);
              if (!p) return null;
              const committed = commitOn && commitX >= p.x && p.state === "delivered";
              const held = p.state === "held";
              const stroke = committed ? "var(--green)" : held ? "var(--amber)" : "var(--blue)";
              const fill = committed ? "var(--green-dim)" : held ? "var(--amber-dim)" : "var(--blue-dim)";
              const born = pulse(t, m.dep, m.dep + 0.05);
              return (
                <g key={m.ts}>
                  {born > 0 && (
                    <circle cx={p.x} cy={p.y} r={14 + born * 10} fill="none"
                      stroke="var(--blue)" strokeWidth="1" opacity={born * 0.7} />
                  )}
                  <circle cx={p.x} cy={p.y} r="14" fill={fill} stroke={stroke} strokeWidth="1.4" />
                  <text x={p.x} y={p.y + 4} fill="var(--ink)" fontSize="11"
                    fontFamily="var(--font-mono)" textAnchor="middle">{m.ts}</text>
                  {committed && (
                    <text x={p.x + 14} y={p.y - 12} fill="var(--green)" fontSize="11"
                      fontFamily="var(--font-mono)">✓</text>
                  )}
                </g>
              );
            })}
          </svg>
        </div>
        <div className="panel-caption" style={{ minHeight: 76 }}>
          {CAPTIONS[Math.max(stageIdx, 0)]}
        </div>
      </div>

      <p className="lede" style={{ marginTop: 28 }}>
        The fabric therefore <strong>already</strong> establishes order, <strong>already</strong>{" "}
        crosses a commit barrier, and <strong>already</strong> replicates — a client can scatter
        a record to all replicas at a single total-order position in one round trip. Three
        mechanisms it runs for communication correctness alone. These are exactly the three
        mechanisms fault tolerance needs.
      </p>
    </Section>
  );
}
