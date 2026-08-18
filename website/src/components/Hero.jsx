import { useRef } from "react";
import { motion } from "motion/react";
import { useLoop, eseg, pulse } from "../lib/anim";

/* Animated Figure 1 of the paper: the same request drawn twice.
   Top: every prior transparent system stacks the durable write after the
   delivery-confirmation wait, holding the reply (+2963 µs measured).
   Bottom: OneBarrier lands the copy to backups inside the commit barrier
   the network crosses anyway, so the reply leaves at the barrier
   (+4.6 µs measured). One dashed line through both rows: one barrier. */

const X0 = 30, BAR0 = 120, BX = 560, WR1 = 852, XEND = 1008;
const P_IN = [0.02, 0.1];      // input flies in
const P_WAIT = [0.1, 0.42];    // both rows traverse the delivery wait
const P_COPY = [0.12, 0.34];   // row 2: the copy lands inside the wait
const P_RIDE = [0.44, 0.58];   // row 2: reply departs at the barrier
const P_WRITE = [0.44, 0.76];  // row 1: the durable write grinds on
const P_STACK = [0.78, 0.9];   // row 1: reply finally departs
const FADE = [0.95, 0.995];

function dotX(uIn, uWait, tail) {
  return X0 + (BAR0 - X0) * uIn + (BX - BAR0) * uWait + tail;
}

function HeroRace() {
  const ref = useRef(null);
  const { t } = useLoop(9000, ref);
  const W = 1080, H = 232;
  const Y1 = 64, Y2 = 154;               // row centers (stack / ride)
  const BH = 34;                          // bar height

  const uIn = eseg(t, ...P_IN);
  const uWait = eseg(t, ...P_WAIT);
  const uCopy = eseg(t, ...P_COPY);
  const uRide = eseg(t, ...P_RIDE);
  const uWrite = eseg(t, ...P_WRITE);
  const uStack = eseg(t, ...P_STACK);
  const fade = 1 - eseg(t, ...FADE);
  const bGlow = pulse(t, 0.38, 0.52);

  const x1 = dotX(uIn, uWait, (WR1 - BX) * uWrite + (XEND - WR1) * uStack);
  const x2 = dotX(uIn, uWait, (XEND - BX) * uRide);
  const show = t >= P_IN[0] ? Math.min((t - P_IN[0]) * 24, 1) * fade : 0;

  const barLabel = (x, y, text, color, size = 11.5) => (
    <text x={x} y={y} fill={color} fontSize={size} fontFamily="var(--font-mono)" textAnchor="middle">
      {text}
    </text>
  );

  return (
    <div ref={ref} aria-hidden="true">
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage" style={{ opacity: 0.95 }}>
        {/* row labels */}
        <text x={BAR0} y={Y1 - 32} fill="var(--red)" fontSize="11" fontFamily="var(--font-mono)">
          prior transparent systems — the write stacks after the wait
        </text>
        <text x={BAR0} y={Y2 - 32} fill="var(--green)" fontSize="11" fontFamily="var(--font-mono)">
          OneBarrier — the write rides inside the same wait
        </text>

        {/* row 1: wait, then the stacked durable write */}
        <rect x={BAR0} y={Y1 - BH / 2} width={BX - BAR0} height={BH} rx="7"
          fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.3" />
        <rect x={BAR0} y={Y1 - BH / 2} width={(BX - BAR0) * uWait} height={BH} rx="7"
          fill="rgba(57,135,229,0.22)" />
        {barLabel((BAR0 + BX) / 2, Y1 + 4, "wait for delivery confirmation", "var(--blue)")}
        <rect x={BX} y={Y1 - BH / 2} width={WR1 - BX} height={BH} rx="7"
          fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.3" />
        <rect x={BX} y={Y1 - BH / 2} width={(WR1 - BX) * uWrite} height={BH} rx="7"
          fill="rgba(230,103,103,0.24)" />
        {barLabel((BX + WR1) / 2, Y1 + 4, "durable write", "var(--red)")}
        {uWrite > 0.02 && uStack < 0.02 && (
          <text x={WR1 + 14} y={Y1 + 4} fill="var(--red)" fontSize="11" fontFamily="var(--font-mono)"
            opacity={(0.55 + 0.45 * Math.sin(t * Math.PI * 10)) * fade}>
            reply held…
          </text>
        )}
        {uStack > 0.1 && (
          <text x={XEND + 20} y={Y1 + 4} fill="var(--red)" fontSize="11" fontFamily="var(--font-mono)" opacity={fade}>
            reply
          </text>
        )}
        <text x={(BX + WR1) / 2} y={Y1 + BH / 2 + 22} fill="var(--red)" fontSize="11.5"
          fontFamily="var(--font-mono)" textAnchor="middle" opacity={eseg(t, 0.8, 0.86) * fade}>
          +2963 µs measured
        </text>

        {/* row 2: the same wait, the copy riding inside it */}
        <rect x={BAR0} y={Y2 - BH / 2} width={BX - BAR0} height={BH} rx="7"
          fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.3" />
        <rect x={BAR0} y={Y2 - BH / 2} width={(BX - BAR0) * uWait} height={BH} rx="7"
          fill="rgba(57,135,229,0.22)" />
        {barLabel((BAR0 + BX) / 2, Y2 + 4, "the same wait (commit barrier)", "var(--blue)")}
        <rect x={BAR0} y={Y2 + BH / 2 + 8} width={300} height={26} rx="6"
          fill="var(--green-dim)" stroke="var(--green)" strokeWidth="1.2" />
        <rect x={BAR0} y={Y2 + BH / 2 + 8} width={300 * uCopy} height={26} rx="6"
          fill="rgba(25,158,112,0.26)" />
        {barLabel(BAR0 + 150, Y2 + BH / 2 + 25, "copy to k−1 backups", "var(--green)", 10.5)}
        {uCopy >= 1 && (
          <text x={BAR0 + 312} y={Y2 + BH / 2 + 25} fill="var(--green)" fontSize="10.5"
            fontFamily="var(--font-mono)" opacity={eseg(t, P_COPY[1], P_COPY[1] + 0.04) * fade}>
            ✓ already durable
          </text>
        )}
        {uRide > 0.1 && (
          <text x={XEND + 20} y={Y2 + 4} fill="var(--green)" fontSize="11" fontFamily="var(--font-mono)" opacity={fade}>
            reply
          </text>
        )}
        <text x={(BX + XEND) / 2} y={Y2 + BH / 2 + 25} fill="var(--green)" fontSize="11.5"
          fontFamily="var(--font-mono)" textAnchor="middle" opacity={eseg(t, 0.5, 0.56) * fade}>
          +4.6 µs measured
        </text>

        {/* the one barrier, through both rows */}
        <line x1={BX} y1={20} x2={BX} y2={H - 26} stroke="var(--amber)" strokeWidth="1.4"
          strokeDasharray="5 5" />
        <line x1={BX} y1={20} x2={BX} y2={H - 26} stroke="var(--amber)" strokeWidth="7"
          opacity={0.12 + 0.3 * bGlow} />
        <text x={BX} y={H - 8} fill="var(--amber)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
          commit barrier — the network confirms delivery here, with or without fault tolerance
        </text>

        {/* the request, racing itself */}
        {[{ x: x1, y: Y1 }, { x: x2, y: Y2 }].map((d, i) => (
          <g key={i} opacity={show}>
            <circle cx={d.x} cy={d.y} r={10}
              fill={i === 0 ? (x1 > BX ? "var(--red-dim)" : "var(--blue-dim)") : (x2 > BX ? "var(--green-dim)" : "var(--blue-dim)")}
              stroke={i === 0 ? (x1 > BX ? "var(--red)" : "var(--blue)") : (x2 > BX ? "var(--green)" : "var(--blue)")}
              strokeWidth="1.5" />
            <text x={d.x} y={d.y + 3.5} fill="var(--ink)" fontSize="9" fontFamily="var(--font-mono)" textAnchor="middle">
              op
            </text>
          </g>
        ))}
      </svg>
    </div>
  );
}

const STATS = [
  { v: "+4.6", u: "µs", k: "durable write riding inside the barrier — vs +2963 µs stacked after it", amber: true },
  { v: "+0", u: "RTT", k: "added by output commit — it is the fabric's own barrier" },
  { v: "5", u: "servers", k: "unmodified redis · memcached · nginx · node.js · postgresql" },
  { v: "0", u: `lost`, k: "of 191,073 acknowledged writes across injected crashes" },
];

export default function Hero() {
  return (
    <header className="hero" id="top">
      <div className="wrap">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, ease: [0.22, 1, 0.36, 1] }}
        >
          <div className="overline">OneBarrier · an interactive companion to the paper</div>
          <h1>
            Transparent fault<br />
            tolerance, <span className="free">for free.</span>
          </h1>
          <p className="hero-sub">
            Crash-recover an <strong>unmodified server</strong> — no code changes, no kernel
            module — by routing it through a microsecond <strong>total-order fabric</strong>.
            The trick: the output-commit barrier that fault tolerance needs and the
            reliable-delivery barrier the fabric already crosses are <strong>the same barrier</strong>.
          </p>
          <div className="hero-meta">
            <span>Bojie Li · Pine AI</span>
            <a href="https://arxiv.org/abs/2608.14601" target="_blank" rel="noreferrer">paper ↗</a>
            <a href="https://github.com/19PINE-AI/OneBarrier" target="_blank" rel="noreferrer">code ↗</a>
            <a href="#reckoning">honest novelty reckoning ↓</a>
          </div>
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 1.2, delay: 0.5 }}
          style={{ marginTop: 48 }}
        >
          <HeroRace />
        </motion.div>

        <motion.div
          className="hero-stats"
          initial={{ opacity: 0, y: 24 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.7, delay: 0.35 }}
        >
          {STATS.map((s) => (
            <div className="hero-stat" key={s.k}>
              <div className={"v" + (s.amber ? " amber" : "")}>
                {s.v}
                <span className="u">{s.u}</span>
              </div>
              <div className="k">{s.k}</div>
            </div>
          ))}
        </motion.div>
      </div>
    </header>
  );
}
