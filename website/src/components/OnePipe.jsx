import { useRef } from "react";
import { Section } from "./ui";
import { PlayerHead, StageChips } from "./ui";
import { useLoop, eseg, lerp, pulse } from "../lib/anim";

/* ---------------- geometry ---------------- */
const W = 960, H = 520;
const SENDERS = [
  { name: "host A", link: "A", x: 96, y: 92 },
  { name: "host B", link: "B", x: 96, y: 210 },
  { name: "host C", link: "C", x: 96, y: 328 },
];
const SW = { cx: 415, cy: 200, x: 338, y: 128, w: 154, h: 144 }; // switch box
const BUF = { cx: 712, yTop: 112, slotH: 33, x: 660, y: 84, w: 104, h: 236 }; // receive buffer
const PIPE = { y: 452, x0: 380, slotW: 62, xStart: 410 };

/* ---------------- message schedule ----------------
   ts 2,9 from A · 5,11 from B · 3,7 from C.
   dep = leaves host · sw = passes switch (register update) · buf = enters buffer */
const MSGS = [
  { ts: 2,  host: 0, dep: 0.100, sw: 0.150, buf: 0.195 },
  { ts: 5,  host: 1, dep: 0.110, sw: 0.145, buf: 0.190 },
  { ts: 3,  host: 2, dep: 0.130, sw: 0.180, buf: 0.225 },
  { ts: 7,  host: 2, dep: 0.160, sw: 0.200, buf: 0.245 },
  { ts: 9,  host: 0, dep: 0.190, sw: 0.240, buf: 0.285 },
  { ts: 11, host: 1, dep: 0.210, sw: 0.250, buf: 0.295 },
];
const ORDER = [2, 3, 5, 7, 9, 11];
const ARRIVAL = [5, 2, 3, 7, 9, 11]; // buffer-arrival order (for ACK stagger)

/* beacons on idle links carry the host clock (12) and raise the registers */
const BEACONS = [
  { host: 0, dep: 0.310, sw: 0.360 },
  { host: 1, dep: 0.320, sw: 0.370 },
  { host: 2, dep: 0.300, sw: 0.350 },
];

/* per-input-link registers R_L: last barrier seen on link L (Eq. 4.1) */
const REG = {
  A: [[0.150, 2], [0.240, 9], [0.360, 12]],
  B: [[0.145, 5], [0.250, 11], [0.370, 12]],
  C: [[0.180, 3], [0.200, 7], [0.350, 12]],
};
function regVal(link, t) {
  let v = null;
  for (const [tt, val] of REG[link]) if (t >= tt) v = val;
  return v;
}
function barrierB(t) {
  const a = regVal("A", t), b = regVal("B", t), c = regVal("C", t);
  if (a == null || b == null || c == null) return null;
  return Math.min(a, b, c);
}

/* reliable mode: ACKs, then commit messages carrying commit barrier T=12 */
const ACK_T = (i) => 0.44 + i * 0.014;          // i = buffer-arrival index
const ACK_D1 = 0.030, ACK_D2 = 0.030;           // buffer→switch, switch→host
const COMMITS = [
  { host: 0, dep: 0.575, sw: 0.620 },
  { host: 1, dep: 0.590, sw: 0.635 },
  { host: 2, dep: 0.605, sw: 0.650 },
];
const C_READY = 0.650;                           // C = min(12,12,12) known
const CLINE_A = 0.655, CLINE_B = 0.705;          // commit barrier travels to receiver
const relT = (ts) => 0.725 + ORDER.indexOf(ts) * 0.032;
const REL_DUR = 0.038;

function commitC(t) {
  if (t < COMMITS[0].sw) return null;
  if (t < C_READY) return "…";
  return 12;
}

const STAGES = [
  { at: 0.0,   label: "stamp" },
  { at: 0.10,  label: "aggregate barrier B" },
  { at: 0.42,  label: "ack (prepare)" },
  { at: 0.56,  label: "commit barrier C" },
  { at: 0.72,  label: "deliver ≤ C" },
];

const CAPTIONS = [
  <>
    <b>① Stamp.</b> Each sender assigns a non-decreasing <b>message timestamp</b> from its
    synchronized local clock — and initializes a second packet field, the{" "}
    <b style={{ color: "var(--blue)" }}>barrier timestamp</b>, to the same value. The message
    ts decides delivery order and is never modified; the barrier ts is what the network will
    rewrite.
  </>,
  <>
    <b>② Aggregate.</b> The switch keeps a register <span className="mono">R_L</span> per input
    link — the last barrier seen on that link — and rewrites every departing packet's barrier
    to <b style={{ color: "var(--blue)" }}>B = min(R_A, R_B, R_C)</b> (Eq. 4.1). B is a
    promise: <i>nothing with ts &lt; B can ever arrive again</i>. Beacons keep idle links
    advancing. The receiver sorts arrivals by message ts; <b>best-effort 1Pipe delivers
    everything below B right here</b>, 0.5 RTT after send.
  </>,
  <>
    <b>③ Prepare.</b> Reliable 1Pipe is a two-phase commit. In the prepare phase the receiver
    only <b>buffers</b> each message and returns an ACK; the sender retransmits losses. Nothing
    is delivered yet — reliability must be settled before the order is released.
  </>,
  <>
    <b>④ Commit.</b> Once a sender holds ACKs for everything ≤ T, it sends a <b>commit
    message</b> carrying <b style={{ color: "var(--amber)" }}>commit barrier T</b>. Switches
    aggregate the <b>minimum commit barrier</b> — the identical min-aggregation that produced
    B, run over commit messages instead of data packets.
  </>,
  <>
    <b>⑤ Deliver.</b> When commit barrier C reaches the receiver, it releases everything
    ≤ C from the buffer — atomically, in message-timestamp order, ~1.5 RTT + barrier wait
    after send. <b>This commit barrier is THE barrier</b>: the one OneBarrier's output-commit
    hold rides for free.
  </>,
];

/* position of a data message at time t */
function msgPos(m, t) {
  const s = SENDERS[m.host];
  const slotY = BUF.yTop + ORDER.indexOf(m.ts) * BUF.slotH; // priority queue: sorted slot
  const finalX = PIPE.xStart + ORDER.indexOf(m.ts) * PIPE.slotW;
  const rel = relT(m.ts);

  if (t < m.dep) return null;
  if (t < m.sw) {
    const u = eseg(t, m.dep, m.sw);
    return { x: lerp(s.x + 46, SW.x - 4, u), y: lerp(s.y, SW.cy, u) - 14 * Math.sin(Math.PI * u), state: "flight" };
  }
  if (t < m.buf) {
    const u = eseg(t, m.sw, m.buf);
    return { x: lerp(SW.x + SW.w + 4, BUF.x - 6, u), y: lerp(SW.cy, slotY, u) - 10 * Math.sin(Math.PI * u), state: "flight" };
  }
  if (t < rel) return { x: BUF.cx, y: slotY, state: "held" };
  if (t < rel + REL_DUR) {
    const u = eseg(t, rel, rel + REL_DUR);
    return { x: lerp(BUF.cx, finalX, u), y: lerp(slotY, PIPE.y, u) + 10 * Math.sin(Math.PI * u), state: "releasing" };
  }
  return { x: finalX, y: PIPE.y, state: "delivered" };
}

export default function OnePipe() {
  const ref = useRef(null);
  const { t, playing, setPlaying, seek } = useLoop(26000, ref);
  const stageIdx = STAGES.findIndex(
    (s, i) => t >= s.at && (i === STAGES.length - 1 || t < STAGES[i + 1].at)
  );
  const B = barrierB(t);
  const C = commitC(t);
  const clineX = lerp(SW.x + SW.w + 8, BUF.x - 8, eseg(t, CLINE_A, CLINE_B));
  const commitAtBuf = t >= CLINE_B;

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
        1Pipe attaches <strong>two timestamps</strong> to every packet. The{" "}
        <strong>message timestamp</strong>, set by the sender, decides delivery order. The{" "}
        <strong style={{ color: "var(--blue)" }}>barrier timestamp B</strong> is aggregated
        hop-by-hop in the switches — the running minimum over all input links — and tells a
        receiver "everything below B has arrived," giving best-effort ordered delivery in
        0.5&thinsp;RTT. <strong>Reliable</strong> mode adds a two-phase commit: buffer + ACK,
        then senders emit a <strong style={{ color: "var(--amber)" }}>commit barrier C</strong>{" "}
        that the switches aggregate the same way — delivery of everything ≤ C becomes atomic,
        at ~1.5&thinsp;RTT, on a 1–2&thinsp;µs RDMA fabric.
      </p>

      <div className="panel" style={{ marginTop: 34 }} ref={ref}>
        <PlayerHead
          title="anim 01 · the 1Pipe fabric — barrier B and commit C"
          playing={playing}
          setPlaying={setPlaying}
        />
        <StageChips stages={STAGES} t={t} seek={seek} />
        <div style={{ padding: "6px 10px 0" }}>
          <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage" role="img"
            aria-label="Animation of the 1Pipe fabric: hosts stamp messages with message and barrier timestamps; the switch aggregates the barrier timestamp B as the minimum over per-link registers; the receiver buffers messages sorted by timestamp; in reliable mode the receiver ACKs, senders emit commit messages, the switch aggregates the minimum commit barrier C, and the receiver atomically delivers everything up to C in timestamp order.">

            {/* links */}
            {SENDERS.map((s) => (
              <line key={s.name} x1={s.x + 46} y1={s.y} x2={SW.x} y2={SW.cy}
                stroke="var(--hairline)" strokeWidth="1" />
            ))}
            <line x1={SW.x + SW.w} y1={SW.cy} x2={BUF.x} y2={SW.cy}
              stroke="var(--hairline)" strokeWidth="1" />

            {/* senders */}
            {SENDERS.map((s) => (
              <g key={s.name}>
                <rect x={s.x - 46} y={s.y - 24} width="92" height="48" rx="7"
                  fill="var(--surface-2)" stroke="var(--hairline)" />
                <text x={s.x} y={s.y - 3} fill="var(--ink)" fontSize="12"
                  fontFamily="var(--font-mono)" textAnchor="middle">{s.name}</text>
                <text x={s.x} y={s.y + 14} fill="var(--muted)" fontSize="9.5"
                  fontFamily="var(--font-mono)" textAnchor="middle">clock ≈ synced</text>
              </g>
            ))}
            <text x={96} y={40} fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
              senders stamp msg ts
            </text>

            {/* switch with per-link registers */}
            <g>
              <rect x={SW.x} y={SW.y} width={SW.w} height={SW.h} rx="10"
                fill="var(--surface-2)" stroke="var(--hairline)" strokeWidth="1.2" />
              <text x={SW.cx} y={SW.y + 22} fill="var(--ink)" fontSize="12"
                fontFamily="var(--font-mono)" textAnchor="middle">programmable switch</text>
              {["A", "B", "C"].map((l, i) => {
                const v = regVal(l, t);
                const fresh = REG[l].some(([tt]) => t >= tt && t < tt + 0.02);
                return (
                  <g key={l}>
                    <rect x={SW.x + 14} y={SW.y + 34 + i * 26} width={SW.w - 28} height="21" rx="4"
                      fill={fresh ? "var(--blue-dim)" : "var(--surface-3)"}
                      stroke={fresh ? "var(--blue)" : "var(--hairline)"} strokeWidth="1" />
                    <text x={SW.x + 24} y={SW.y + 49 + i * 26} fill="var(--ink-2)" fontSize="11"
                      fontFamily="var(--font-mono)">
                      R_{l} = {v == null ? "–" : v}
                    </text>
                    <text x={SW.x + SW.w - 24} y={SW.y + 49 + i * 26} fill="var(--muted)" fontSize="9"
                      fontFamily="var(--font-mono)" textAnchor="end">link {l}</text>
                  </g>
                );
              })}
              <text x={SW.cx} y={SW.y + SW.h - 12} fill="var(--muted)" fontSize="9.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                R_L := last barrier on link L
              </text>

              {/* barrier B readout */}
              <rect x={SW.cx - 124} y={SW.y + SW.h + 12} width="248" height="26" rx="5"
                fill="var(--blue-dim)" stroke="rgba(57,135,229,0.5)" />
              <text x={SW.cx} y={SW.y + SW.h + 29} fill="var(--blue)" fontSize="11"
                fontFamily="var(--font-mono)" textAnchor="middle">
                {B == null
                  ? "barrier B = min(R) — waiting for all links"
                  : `barrier B = min(${regVal("A", t)},${regVal("B", t)},${regVal("C", t)}) = ${B}`}
              </text>
              {/* commit C readout */}
              <rect x={SW.cx - 124} y={SW.y + SW.h + 44} width="248" height="26" rx="5"
                fill={C != null ? "var(--amber-dim)" : "var(--surface-3)"}
                stroke={C != null ? "rgba(242,177,61,0.5)" : "var(--hairline)"} />
              <text x={SW.cx} y={SW.y + SW.h + 61}
                fill={C != null ? "var(--amber)" : "var(--muted)"} fontSize="11"
                fontFamily="var(--font-mono)" textAnchor="middle">
                {C == null ? "commit C = – (reliable mode)"
                  : C === "…" ? "commit C = min(T…) aggregating"
                  : `commit C = min(12,12,12) = ${C}`}
              </text>
            </g>

            {/* receive buffer (priority queue) */}
            <g>
              <rect x={BUF.x} y={BUF.y} width={BUF.w} height={BUF.h} rx="8"
                fill="var(--surface-2)" stroke="var(--hairline)" />
              <text x={BUF.x + BUF.w / 2} y={BUF.y - 24} fill="var(--ink)" fontSize="11.5"
                fontFamily="var(--font-mono)" textAnchor="middle">receive buffer</text>
              <text x={BUF.x + BUF.w / 2} y={BUF.y - 10} fill="var(--muted)" fontSize="9.5"
                fontFamily="var(--font-mono)" textAnchor="middle">sorted by msg ts</text>
              <text x={BUF.x + BUF.w / 2} y={BUF.y + BUF.h + 16} fill="var(--muted)" fontSize="9.5"
                fontFamily="var(--font-mono)" textAnchor="middle">
                {stageIdx <= 1 ? "best-effort: deliver ≤ B" : "reliable: hold for commit C"}
              </text>
            </g>

            {/* delivered pipe */}
            <g>
              <rect x={PIPE.x0} y={PIPE.y - 22} width={W - PIPE.x0 - 16} height="44" rx="22"
                fill="none" stroke="var(--hairline)" strokeWidth="1.2" />
              <text x={PIPE.x0 + 4} y={PIPE.y - 34} fill="var(--muted)" fontSize="10.5"
                fontFamily="var(--font-mono)">
                delivered ≤ C — one total order, atomic, every receiver
              </text>
            </g>

            {/* beacons */}
            {BEACONS.map((b) => {
              if (t < b.dep || t > b.sw) return null;
              const s = SENDERS[b.host];
              const u = eseg(t, b.dep, b.sw);
              const x = lerp(s.x + 46, SW.x - 4, u);
              const y = lerp(s.y, SW.cy, u) - 14 * Math.sin(Math.PI * u);
              return (
                <g key={b.host} opacity="0.9">
                  <circle cx={x} cy={y} r="11" fill="none" stroke="var(--blue)"
                    strokeWidth="1.2" strokeDasharray="3 3" />
                  <text x={x} y={y + 3.5} fill="var(--blue)" fontSize="9"
                    fontFamily="var(--font-mono)" textAnchor="middle">12</text>
                  <text x={x} y={y - 16} fill="var(--muted)" fontSize="8.5"
                    fontFamily="var(--font-mono)" textAnchor="middle">beacon</text>
                </g>
              );
            })}

            {/* ACKs (prepare phase) */}
            {ARRIVAL.map((ts, i) => {
              const m = MSGS.find((x) => x.ts === ts);
              const a0 = ACK_T(i);
              if (t < a0 || t > a0 + ACK_D1 + ACK_D2) return null;
              const s = SENDERS[m.host];
              const slotY = BUF.yTop + ORDER.indexOf(ts) * BUF.slotH;
              let x, y;
              if (t < a0 + ACK_D1) {
                const u = eseg(t, a0, a0 + ACK_D1);
                x = lerp(BUF.x - 6, SW.x + SW.w + 4, u);
                y = lerp(slotY, SW.cy, u) + 12 * Math.sin(Math.PI * u);
              } else {
                const u = eseg(t, a0 + ACK_D1, a0 + ACK_D1 + ACK_D2);
                x = lerp(SW.x - 4, s.x + 46, u);
                y = lerp(SW.cy, s.y, u) + 12 * Math.sin(Math.PI * u);
              }
              return (
                <g key={ts}>
                  <circle cx={x} cy={y} r="7" fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.2" />
                  <text x={x} y={y + 3} fill="var(--green)" fontSize="7.5"
                    fontFamily="var(--font-mono)" textAnchor="middle">ack</text>
                </g>
              );
            })}

            {/* commit messages */}
            {COMMITS.map((c) => {
              if (t < c.dep || t > c.sw) return null;
              const s = SENDERS[c.host];
              const u = eseg(t, c.dep, c.sw);
              const x = lerp(s.x + 46, SW.x - 4, u);
              const y = lerp(s.y, SW.cy, u) - 14 * Math.sin(Math.PI * u);
              return (
                <g key={c.host}>
                  <rect x={x - 8} y={y - 8} width="16" height="16" rx="3"
                    transform={`rotate(45 ${x} ${y})`}
                    fill="var(--amber-dim)" stroke="var(--amber)" strokeWidth="1.3" />
                  <text x={x} y={y - 14} fill="var(--amber)" fontSize="9"
                    fontFamily="var(--font-mono)" textAnchor="middle">T=12</text>
                </g>
              );
            })}

            {/* commit barrier line traveling to the receiver */}
            {t >= CLINE_A && (
              <g>
                <line x1={Math.min(clineX, BUF.x - 8)} y1={BUF.y - 6}
                  x2={Math.min(clineX, BUF.x - 8)} y2={BUF.y + BUF.h + 6}
                  stroke="var(--amber)" strokeWidth="2" />
                <line x1={Math.min(clineX, BUF.x - 8)} y1={BUF.y - 6}
                  x2={Math.min(clineX, BUF.x - 8)} y2={BUF.y + BUF.h + 6}
                  stroke="var(--amber)" strokeWidth="8"
                  opacity={commitAtBuf ? 0.12 : 0.25} />
                <text x={Math.min(clineX, BUF.x - 8) - 8} y={BUF.y + 8} fill="var(--amber)"
                  fontSize="10" fontFamily="var(--font-mono)" textAnchor="end">
                  commit barrier C={C === null || C === "…" ? "" : C}
                </text>
              </g>
            )}

            {/* data messages */}
            {MSGS.map((m) => {
              const p = msgPos(m, t);
              if (!p) return null;
              const held = p.state === "held";
              const done = p.state === "delivered" || p.state === "releasing";
              const underB = B != null && B >= m.ts;
              const stroke = done ? "var(--green)"
                : held && commitAtBuf ? "var(--amber)"
                : held && underB ? "var(--blue)"
                : held ? "var(--muted)"
                : "var(--blue)";
              const fill = done ? "var(--green-dim)"
                : held && commitAtBuf ? "var(--amber-dim)"
                : "var(--blue-dim)";
              const born = pulse(t, m.dep, m.dep + 0.04);
              return (
                <g key={m.ts}>
                  {born > 0 && (
                    <circle cx={p.x} cy={p.y} r={14 + born * 10} fill="none"
                      stroke="var(--blue)" strokeWidth="1" opacity={born * 0.7} />
                  )}
                  <circle cx={p.x} cy={p.y} r="14" fill={fill} stroke={stroke} strokeWidth="1.4" />
                  <text x={p.x} y={p.y + 4} fill="var(--ink)" fontSize="11"
                    fontFamily="var(--font-mono)" textAnchor="middle">{m.ts}</text>
                  {held && underB && !commitAtBuf && (
                    <text x={p.x + 22} y={p.y + 3.5} fill="var(--blue)" fontSize="8.5"
                      fontFamily="var(--font-mono)">≤B</text>
                  )}
                  {p.state === "delivered" && (
                    <text x={p.x + 14} y={p.y - 12} fill="var(--green)" fontSize="11"
                      fontFamily="var(--font-mono)">✓</text>
                  )}
                </g>
              );
            })}
          </svg>
        </div>
        <div className="panel-caption" style={{ minHeight: 96 }}>
          {CAPTIONS[Math.max(stageIdx, 0)]}
        </div>
      </div>

      <p className="lede" style={{ marginTop: 28 }}>
        Two timestamps, one mechanism: the switches aggregate a running minimum, first over
        data packets and beacons (<strong style={{ color: "var(--blue)" }}>B</strong>, the
        order), then over commit messages (<strong style={{ color: "var(--amber)" }}>C</strong>,
        the reliability). The fabric therefore <strong>already</strong> establishes order,{" "}
        <strong>already</strong> crosses a commit barrier, and <strong>already</strong>{" "}
        replicates — a scattering delivers a record to all replicas at a single total-order
        position in one round trip. Three mechanisms it runs for communication correctness
        alone. These are exactly the three mechanisms fault tolerance needs.
      </p>
    </Section>
  );
}
