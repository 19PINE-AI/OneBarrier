import { useRef } from "react";
import { Section, Reveal, PlayerHead } from "./ui";
import { useLoop, seg, eseg, lerp, pulse } from "../lib/anim";

/* ================= architecture stack ================= */

function StackDiagram() {
  const layers = [
    {
      label: "unmodified server binary",
      sub: "redis · memcached · nginx · node.js · postgresql — zero code changes",
      color: "var(--ink)", stroke: "var(--hairline)", fill: "var(--surface-2)",
    },
    {
      label: "determinism libOS  (LD_PRELOAD, ~900 lines of C)",
      sub: "virtual clock · seeded randomness · share-nothing shards · output suppression",
      color: "var(--green)", stroke: "rgba(25,158,112,0.5)", fill: "var(--green-dim)",
    },
    {
      label: "1Pipe total-order reliable fabric",
      sub: "global order · reliable-delivery commit barrier · 1-RTT replication",
      color: "var(--blue)", stroke: "rgba(57,135,229,0.5)", fill: "var(--blue-dim)",
    },
  ];
  return (
    <svg viewBox="0 0 960 330" className="svg-stage" role="img"
      aria-label="OneBarrier architecture: an unmodified server over the determinism libOS over the 1Pipe fabric, with in-fabric replicas holding the recovery log.">
      {layers.map((l, i) => (
        <g key={l.label}>
          <rect x="60" y={30 + i * 92} width="560" height="74" rx="9"
            fill={l.fill} stroke={l.stroke} strokeWidth="1.2" />
          <text x="88" y={62 + i * 92} fill={l.color} fontSize="15" fontFamily="var(--font-mono)" fontWeight="600">
            {l.label}
          </text>
          <text x="88" y="0" transform={`translate(0 ${84 + i * 92})`} fill="var(--ink-2)" fontSize="12" fontFamily="var(--font-body)">
            {l.sub}
          </text>
          {i < 2 && (
            <g stroke="var(--muted)" strokeWidth="1.2">
              <line x1="340" y1={104 + i * 92} x2="340" y2={122 + i * 92} />
              <polyline points={`335,${116 + i * 92} 340,${122 + i * 92} 345,${116 + i * 92}`} fill="none" />
            </g>
          )}
        </g>
      ))}
      {/* intercept annotation */}
      <text x="632" y="70" fill="var(--muted)" fontSize="11.5" fontFamily="var(--font-mono)">← POSIX surface intercepted:</text>
      <text x="648" y="88" fill="var(--muted)" fontSize="11.5" fontFamily="var(--font-mono)">sockets → fabric</text>
      <text x="648" y="104" fill="var(--muted)" fontSize="11.5" fontFamily="var(--font-mono)">time, rng, threads → virtualized</text>

      {/* replicas */}
      {[0, 1].map((i) => (
        <g key={i}>
          <rect x={700 + i * 10} y={216 + i * 8} width="180" height="66" rx="9"
            fill="var(--surface-2)" stroke="var(--hairline)" />
        </g>
      ))}
      <rect x="690" y="208" width="180" height="66" rx="9"
        fill="var(--surface-3)" stroke="rgba(25,158,112,0.5)" />
      <text x="780" y="236" fill="var(--green)" fontSize="12.5" fontFamily="var(--font-mono)" textAnchor="middle">in-fabric replicas</text>
      <text x="780" y="256" fill="var(--ink-2)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">recovery log, k copies</text>
      {/* scatter arrow */}
      <g stroke="var(--green)" strokeWidth="1.4" fill="none">
        <path d="M 622 250 C 650 250 660 244 686 240" strokeDasharray="5 4" />
        <polyline points="678,235 688,240 679,246" />
      </g>
      <text x="780" y="300" fill="var(--green)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
        each input scattered at its
      </text>
      <text x="780" y="316" fill="var(--green)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
        total-order position · 1 RTT
      </text>
    </svg>
  );
}

/* ================= barrier coincidence timing diagram ================= */

const TW = 960, TH = 470;
const X0 = 110;
const xRTT = (r) => X0 + r * 360;      // 1.5 RTT -> 650
const BREAK_X = 700;                    // axis break for out-of-regime serial write
const SERIAL_X0 = 736, SERIAL_X1 = 858; // drawn extent of the ~100µs write

const LANES = [
  { y: 120, name: "reliable fabric, no FT", sub: "the baseline every message pays" },
  { y: 235, name: "OneBarrier — durability rides", sub: "recovery-log scatter, 1 RTT" },
  { y: 350, name: "serial durability — stacks", sub: "fsync after the barrier (Remus’s mistake)" },
];

/** map loop t to a cursor x. Cursor sweeps 0→1.5RTT over [0,0.55], then to serial end over [0.62,0.9] */
function cursorX(t) {
  if (t < 0.55) return lerp(xRTT(0), xRTT(1.5), eseg(t, 0.06, 0.55));
  if (t < 0.62) return xRTT(1.5);
  return lerp(xRTT(1.5), SERIAL_X1 + 14, eseg(t, 0.62, 0.9));
}

function Bar({ x0, x1, y, h = 22, color, dim, cx, label }) {
  const w = Math.max(0, Math.min(cx, x1) - x0);
  return (
    <g>
      <rect x={x0} y={y - h / 2} width={x1 - x0} height={h} rx="4"
        fill="none" stroke={color} strokeWidth="1" opacity="0.35" strokeDasharray="3 3" />
      {w > 0 && <rect x={x0} y={y - h / 2} width={w} height={h} rx="4" fill={dim} stroke={color} strokeWidth="1.2" />}
      {label && w > 8 && (
        <text x={x0 + 10} y={y + 4} fill={color} fontSize="11" fontFamily="var(--font-mono)">{label}</text>
      )}
    </g>
  );
}

function BarrierAnim() {
  const ref = useRef(null);
  const { t, playing, setPlaying } = useLoop(14000, ref);
  const cx = cursorX(t);
  const barrierReached = cx >= xRTT(1.5) - 0.5;
  const durableAt1 = cx >= xRTT(1);
  const serialDone = cx >= SERIAL_X1;

  return (
    <div className="panel" style={{ marginTop: 34 }} ref={ref}>
      <PlayerHead
        title="anim 02 · the barrier coincidence — ride vs. stack"
        playing={playing}
        setPlaying={setPlaying}
      />
      <div style={{ padding: "10px 10px 0" }}>
        <svg viewBox={`0 0 ${TW} ${TH}`} className="svg-stage" role="img"
          aria-label="Timing diagram: the 1-RTT recovery-log replication completes inside the 1.5-RTT reliable-delivery commit barrier, so OneBarrier releases output at the same barrier as the no-FT baseline; serial durability instead stacks about 100 microseconds after the barrier.">
          {/* time axis */}
          <line x1={X0 - 30} y1={TH - 46} x2={TW - 40} y2={TH - 46} stroke="var(--muted)" strokeWidth="1" />
          {[0, 0.5, 1, 1.5].map((r) => (
            <g key={r}>
              <line x1={xRTT(r)} y1={TH - 52} x2={xRTT(r)} y2={TH - 40} stroke="var(--muted)" strokeWidth="1" />
              <text x={xRTT(r)} y={TH - 24} fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
                {r === 0 ? "0" : `${r} RTT`}
              </text>
            </g>
          ))}
          <text x={xRTT(1.5)} y={TH - 8} fill="var(--ink-2)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
            ≈21 µs at 1Pipe’s operating point
          </text>
          {/* axis break */}
          <g stroke="var(--muted)" strokeWidth="1">
            <line x1={BREAK_X - 5} y1={TH - 52} x2={BREAK_X + 3} y2={TH - 40} />
            <line x1={BREAK_X + 3} y1={TH - 52} x2={BREAK_X + 11} y2={TH - 40} />
          </g>
          <text x={(SERIAL_X0 + SERIAL_X1) / 2} y={TH - 24} fill="var(--red)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
            … ~100 µs
          </text>

          {/* lane labels */}
          {LANES.map((l) => (
            <g key={l.name}>
              <text x={X0 - 30} y={l.y - 30} fill="var(--ink)" fontSize="12.5" fontFamily="var(--font-mono)">{l.name}</text>
              <text x={X0 - 30} y={l.y - 30} dy="16" fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)">{l.sub}</text>
            </g>
          ))}

          {/* lane 0: baseline barrier */}
          <Bar x0={xRTT(0)} x1={xRTT(1.5)} y={LANES[0].y + 8} color="var(--blue)" dim="var(--blue-dim)" cx={cx}
            label="reliable-delivery commit barrier · 1.5 RTT" />

          {/* lane 1: barrier + scatter riding under it */}
          <Bar x0={xRTT(0)} x1={xRTT(1.5)} y={LANES[1].y + 2} color="var(--blue)" dim="var(--blue-dim)" cx={cx}
            label="same barrier — paid anyway" />
          <Bar x0={xRTT(0)} x1={xRTT(1)} y={LANES[1].y + 26} h={14} color="var(--green)" dim="var(--green-dim)" cx={cx}
            label={cx > xRTT(0.55) ? "log scatter · 1 RTT" : ""} />
          {durableAt1 && (
            <g>
              <circle cx={xRTT(1)} cy={LANES[1].y + 26} r={5 + pulse(seg(t, 0, 1), 0.36, 0.5) * 6}
                fill="none" stroke="var(--green)" strokeWidth="1.4" />
              <text x={xRTT(1)} y={LANES[1].y + 54} fill="var(--green)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
                durable here — before the barrier
              </text>
            </g>
          )}

          {/* lane 2: barrier then serial write stacking */}
          <Bar x0={xRTT(0)} x1={xRTT(1.5)} y={LANES[2].y + 2} color="var(--blue)" dim="var(--blue-dim)" cx={cx} />
          <Bar x0={xRTT(1.5)} x1={BREAK_X - 8} y={LANES[2].y + 2} color="var(--red)" dim="var(--red-dim)" cx={cx} />
          <Bar x0={SERIAL_X0} x1={SERIAL_X1} y={LANES[2].y + 2} color="var(--red)" dim="var(--red-dim)" cx={cx}
            label={cx > SERIAL_X0 + 20 ? "serial durable write" : ""} />

          {/* the commit barrier line */}
          {barrierReached && (
            <g>
              <line x1={xRTT(1.5)} y1={62} x2={xRTT(1.5)} y2={TH - 56} stroke="var(--amber)" strokeWidth="2" />
              <line x1={xRTT(1.5)} y1={62} x2={xRTT(1.5)} y2={TH - 56} stroke="var(--amber)" strokeWidth="9" opacity="0.16" />
              <text x={xRTT(1.5)} y={48} fill="var(--amber)" fontSize="12" fontFamily="var(--font-mono)" textAnchor="middle" fontWeight="600">
                THE barrier
              </text>
            </g>
          )}

          {/* output releases */}
          {barrierReached && [0, 1].map((i) => (
            <g key={i}>
              <rect x={xRTT(1.5) + 12} y={LANES[i].y - (i === 0 ? -0 : 6) - 8} width="26" height="18" rx="3"
                fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.2" />
              <text x={xRTT(1.5) + 48} y={LANES[i].y + 6 - (i === 0 ? 0 : 6)} fill="var(--green)" fontSize="11.5" fontFamily="var(--font-mono)">
                {i === 0 ? "output released" : "output released — same instant · +0 RTT"}
              </text>
            </g>
          ))}
          {serialDone && (
            <g>
              <rect x={SERIAL_X1 + 8} y={LANES[2].y - 6} width="26" height="18" rx="3"
                fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.2" />
              <text x={SERIAL_X1 + 20} y={LANES[2].y + 34} fill="var(--red)" fontSize="11.5" fontFamily="var(--font-mono)" textAnchor="end">
                released late — commit 2×
              </text>
            </g>
          )}

          {/* cursor */}
          <line x1={cx} y1={56} x2={cx} y2={TH - 46} stroke="rgba(240,239,233,0.35)" strokeWidth="1" />
          <text x={cx} y={TH - 58} fill="var(--ink-2)" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="middle">t</text>
        </svg>
      </div>
      <div className="panel-caption">
        <b>These are not two barriers that overlap — they are the same barrier.</b>{" "}
        The recovery-log scatter (1&thinsp;RTT, green) completes strictly inside the
        reliable-delivery commit barrier (1.5&thinsp;RTT, blue) that the fabric crosses for
        communication correctness whether or not the system is fault-tolerant. So the reply is
        already safe to release the moment the fabric would release it anyway: fault tolerance
        adds <b>zero round trips</b>, independent of the absolute RTT. Serial durability
        (red) cannot ride — it stacks ~100&thinsp;µs (measured: ~3&thinsp;ms of fsync) on
        every reply, the output-hold failure mode that sank Remus.
      </div>
    </div>
  );
}

/* ================= the three collapses ================= */

const COLLAPSES = [
  {
    eq: "(a) order ⇒ no order-log",
    title: "The order lives in the fabric",
    body: "1Pipe delivers in one global timestamp order, so a recovered replica just re-applies inputs in msg_ts order. The dominant cost of deterministic replay is removed by relocation, not optimization.",
  },
  {
    eq: "(b) snapshot ⇒ ts ≤ T",
    title: "An empty-channel cut",
    body: "To checkpoint, every node independently quiesces at barrier timestamp T: stop dispatching ts > T, drain ts ≤ T, snapshot. Same predicate on both sides of every channel — consistent with no markers and no channel state. Strictly simpler than Chandy–Lamport.",
  },
  {
    eq: "(c) output commit = barrier",
    title: "The one new engineering idea",
    body: "Replicate the recovery log by scattering each input to the backups at its total-order position (1 RTT), and hold replies until the commit barrier — which the fabric crosses anyway. FT’s margin is zero by construction.",
  },
];

export default function Barrier() {
  return (
    <Section
      id="onebarrier"
      ts="003"
      kicker="§3 · OneBarrier: FT as a byproduct"
      title={
        <>
          One barrier: the fabric pre-pays, for communication
          correctness, exactly what fault tolerance needs.
        </>
      }
    >
      <p className="lede">
        An unmodified share-nothing server runs over the determinism libOS, which intercepts its
        POSIX surface: sockets are routed to the fabric, and local non-determinism is
        virtualized so execution becomes a <strong>deterministic function of the fabric-ordered
        input sequence</strong>. The fabric supplies the order and the commit barrier; in-fabric
        replicas supply durability.
      </p>

      <Reveal>
        <div className="panel" style={{ marginTop: 34 }}>
          <div className="panel-head"><span className="title">the OneBarrier stack</span></div>
          <div style={{ padding: "18px 10px 4px" }}>
            <StackDiagram />
          </div>
        </div>
      </Reveal>

      <div className="collapses">
        {COLLAPSES.map((c, i) => (
          <Reveal key={c.eq} delay={i * 0.1}>
            <div className="panel collapse-card">
              <div className="eq">{c.eq}</div>
              <h3>{c.title}</h3>
              <p>{c.body}</p>
            </div>
          </Reveal>
        ))}
      </div>

      <BarrierAnim />
    </Section>
  );
}
