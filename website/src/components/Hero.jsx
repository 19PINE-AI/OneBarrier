import { useRef } from "react";
import { motion } from "motion/react";
import { useLoop, eseg, lerp, pulse } from "../lib/anim";

/* Six messages per loop. Left of the blue line: concurrent, arrival order.
   Past barrier B: sorted into one timestamp order (best-effort delivery).
   Past commit C: the whole ordered group crosses together — atomic, reliable. */
const MSGS = [
  { ts: 2, lane: -34, dep: 0.02 },
  { ts: 5, lane: 22, dep: 0.05 },
  { ts: 3, lane: 8, dep: 0.09 },
  { ts: 9, lane: -18, dep: 0.13 },
  { ts: 7, lane: 30, dep: 0.18 },
  { ts: 11, lane: -8, dep: 0.24 },
];
const ORDER = [2, 3, 5, 7, 9, 11];
const F1 = 0.22, F2 = 0.12;            // flight to B, then settle into sorted slot
const G0 = 0.64, G1 = 0.80;            // the ordered group crosses C together
const FADE = 0.94;

function HeroPipe() {
  const ref = useRef(null);
  const { t } = useLoop(11000, ref);
  const W = 1080, H = 150, mid = H / 2;
  const bx = W * 0.40;                  // barrier timestamp B (ordering)
  const cx = W * 0.72;                  // commit barrier C (reliable delivery)
  const midSlot = (s) => bx + 44 + s * 50;
  const endSlot = (s) => cx + 40 + s * 44;

  return (
    <div ref={ref} aria-hidden="true">
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage" style={{ opacity: 0.95 }}>
        {/* the pipe */}
        <line x1="0" y1={mid} x2={W} y2={mid} stroke="rgba(240,239,233,0.12)" strokeWidth="1" />
        <text x="8" y={mid - 44} fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)">
          concurrent messages, arrival order
        </text>
        <text x={(bx + cx) / 2} y={H - 6} fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
          ordered ≤ B · held for commit
        </text>
        <text x={W - 8} y={H - 6} fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="end">
          delivered ≤ C · atomic
        </text>

        {/* barrier timestamp B — ordering */}
        <line x1={bx} y1={16} x2={bx} y2={H - 16} stroke="var(--blue)" strokeWidth="1.5" />
        <line x1={bx} y1={16} x2={bx} y2={H - 16} stroke="var(--blue)" strokeWidth="7"
          opacity={0.18 + 0.14 * Math.sin(t * Math.PI * 6)} />
        <text x={bx} y={12} fill="var(--blue)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
          barrier ts B · order
        </text>

        {/* commit barrier C — reliable delivery */}
        <line x1={cx} y1={16} x2={cx} y2={H - 16} stroke="var(--amber)" strokeWidth="1.5" />
        <line x1={cx} y1={16} x2={cx} y2={H - 16} stroke="var(--amber)" strokeWidth="7"
          opacity={0.25 + 0.2 * Math.sin(t * Math.PI * 6)} />
        <text x={cx} y={12} fill="var(--amber)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="middle">
          commit barrier C · deliver
        </text>

        {MSGS.map((m) => {
          const slot = ORDER.indexOf(m.ts);
          const u = eseg(t, m.dep, m.dep + F1);          // drift toward B
          const sortU = eseg(t, m.dep + F1, m.dep + F1 + F2); // cross B, snap into ts order
          const groupU = eseg(t, G0, G1);                // whole group crosses C together
          if (t < m.dep) return null;

          const xB = lerp(lerp(-24, bx, u), midSlot(slot), sortU);
          const x = lerp(xB, endSlot(slot), groupU);
          const y = mid + m.lane * (1 - sortU);
          const ordered = sortU >= 1 - 1e-4;
          const committed = x >= cx;
          const bFlash = pulse(t, m.dep + F1, m.dep + F1 + F2);
          const cFlash = Math.max(0, 1 - Math.abs(x - cx) / 34) * (groupU > 0 && groupU < 1 ? 1 : 0);
          const fade = 1 - eseg(t, FADE, 0.995);

          return (
            <g key={m.ts} opacity={Math.min((t - m.dep) * 24, 1) * fade}>
              <circle
                cx={x} cy={y} r={13}
                fill={committed ? "rgba(25,158,112,0.18)" : "rgba(57,135,229,0.16)"}
                stroke={committed ? "var(--green)" : ordered ? "var(--blue)" : "rgba(57,135,229,0.55)"}
                strokeWidth={ordered && !committed ? 1.6 : 1.3}
                strokeDasharray={!ordered ? "3 3" : "none"}
              />
              {bFlash > 0 && (
                <circle cx={x} cy={y} r={13 + bFlash * 8} fill="none" stroke="var(--blue)"
                  strokeWidth="1" opacity={bFlash * 0.8} />
              )}
              {cFlash > 0 && (
                <circle cx={x} cy={y} r={13 + cFlash * 9} fill="none" stroke="var(--amber)"
                  strokeWidth="1" opacity={cFlash * 0.85} />
              )}
              <text x={x} y={y + 3.5} fill="var(--ink)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">
                {m.ts}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

const STATS = [
  { v: "0.23", u: "%", k: "marginal cost of fault tolerance on the critical path", amber: true },
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
            <a href="https://github.com/bojieli/OneBarrier" target="_blank" rel="noreferrer">code ↗</a>
            <a href="#reckoning">honest novelty reckoning ↓</a>
          </div>
        </motion.div>

        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 1.2, delay: 0.5 }}
          style={{ marginTop: 48 }}
        >
          <HeroPipe />
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
