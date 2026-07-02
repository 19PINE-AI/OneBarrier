import { useRef, useState } from "react";

/* ---------- shared tooltip ---------- */
function useTip() {
  const ref = useRef(null);
  const [tip, setTip] = useState(null);
  const show = (e, content) => {
    const r = ref.current.getBoundingClientRect();
    setTip({ x: e.clientX - r.left, y: e.clientY - r.top, content });
  };
  return { ref, tip, show, hide: () => setTip(null) };
}

function TipBox({ tip }) {
  if (!tip) return null;
  return (
    <div className="tooltip" style={{ left: tip.x, top: tip.y }}>
      {tip.content}
    </div>
  );
}

const MONO = "var(--font-mono)";
const fmt = (v) => v.toLocaleString("en-US");

/* ---------- vertical bar chart ---------- */
export function BarChartV({ data, unit, refLine, height = 270 }) {
  const { ref, tip, show, hide } = useTip();
  const W = 520, H = height, padL = 14, padR = 10, padB = 44, padT = 26;
  const max = Math.max(...data.map((d) => d.value)) * 1.18;
  const iw = (W - padL - padR) / data.length;
  const bw = Math.min(iw * 0.56, 54);
  const y = (v) => padT + (H - padT - padB) * (1 - v / max);

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage">
        {/* hairline grid */}
        {[0.25, 0.5, 0.75, 1].map((g) => (
          <line key={g} x1={padL} x2={W - padR} y1={y(max * g / 1.18)} y2={y(max * g / 1.18)}
            stroke="var(--hairline-soft)" strokeWidth="1" />
        ))}
        <line x1={padL} x2={W - padR} y1={y(0)} y2={y(0)} stroke="var(--hairline)" strokeWidth="1" />
        {refLine && (
          <g>
            <line x1={padL} x2={W - padR} y1={y(refLine.value)} y2={y(refLine.value)}
              stroke="var(--blue)" strokeWidth="1" strokeDasharray="3 4" opacity="0.7" />
            <text x={padL + 4} y={y(refLine.value) - 6} fill="var(--blue)" fontSize="10"
              fontFamily={MONO}>{refLine.label}</text>
          </g>
        )}
        {data.map((d, i) => {
          const x = padL + iw * i + (iw - bw) / 2;
          return (
            <g key={d.label}>
              <rect
                x={x} y={y(d.value)} width={bw} height={y(0) - y(d.value)} rx="4"
                fill={d.color} opacity="0.88"
                onMouseMove={(e) => show(e, <><span className="k">{d.label}&nbsp;</span>{fmt(d.value)} {unit}</>)}
                onMouseLeave={hide}
              />
              <text x={x + bw / 2} y={y(d.value) - 7} fill="var(--ink-2)" fontSize="10.5"
                fontFamily={MONO} textAnchor="middle">{fmt(d.value)}</text>
              <text x={x + bw / 2} y={H - padB + 16} fill="var(--muted)" fontSize="10"
                fontFamily={MONO} textAnchor="middle">
                {d.label.split(" ").map((s, j) => (
                  <tspan key={j} x={x + bw / 2} dy={j === 0 ? 0 : 11}>{s}</tspan>
                ))}
              </text>
            </g>
          );
        })}
      </svg>
      <TipBox tip={tip} />
    </div>
  );
}

/* ---------- grouped bars (two series) ---------- */
export function GroupedBars({ groups, series, unit, xLabel, notes, height = 280 }) {
  const { ref, tip, show, hide } = useTip();
  const W = 520, H = height, padL = 14, padR = 10, padB = 40, padT = 30;
  const max = Math.max(...series.flatMap((s) => s.values)) * 1.2;
  const iw = (W - padL - padR) / groups.length;
  const bw = Math.min(iw * 0.3, 34);
  const y = (v) => padT + (H - padT - padB) * (1 - v / max);

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage">
        {[0.25, 0.5, 0.75, 1].map((g) => (
          <line key={g} x1={padL} x2={W - padR} y1={y(max * g / 1.2)} y2={y(max * g / 1.2)}
            stroke="var(--hairline-soft)" strokeWidth="1" />
        ))}
        <line x1={padL} x2={W - padR} y1={y(0)} y2={y(0)} stroke="var(--hairline)" strokeWidth="1" />
        {groups.map((g, i) => (
          <g key={g}>
            {series.map((s, k) => {
              const x = padL + iw * i + iw / 2 + (k - series.length / 2) * (bw + 2);
              return (
                <rect key={s.name} x={x} y={y(s.values[i])} width={bw}
                  height={y(0) - y(s.values[i])} rx="4" fill={s.color} opacity="0.88"
                  onMouseMove={(e) => show(e, <><span className="k">{s.name} · {g} replicas&nbsp;</span>{s.values[i]} {unit}</>)}
                  onMouseLeave={hide}
                />
              );
            })}
            {notes && (
              <text x={padL + iw * i + iw / 2} y={y(Math.max(...series.map((s) => s.values[i]))) - 8}
                fill="var(--green)" fontSize="10.5" fontFamily={MONO} textAnchor="middle">
                {notes[i]}
              </text>
            )}
            <text x={padL + iw * i + iw / 2} y={H - padB + 16} fill="var(--muted)" fontSize="10.5"
              fontFamily={MONO} textAnchor="middle">{g}</text>
          </g>
        ))}
        <text x={W / 2} y={H - 6} fill="var(--muted)" fontSize="10.5" fontFamily={MONO} textAnchor="middle">
          {xLabel}
        </text>
        {/* legend */}
        {series.map((s, k) => (
          <g key={s.name}>
            <rect x={padL + 4 + k * 210} y={6} width="10" height="10" rx="2" fill={s.color} />
            <text x={padL + 20 + k * 210} y={15} fill="var(--ink-2)" fontSize="10.5" fontFamily={MONO}>
              {s.name}
            </text>
          </g>
        ))}
      </svg>
      <TipBox tip={tip} />
    </div>
  );
}

/* ---------- line chart with fit ---------- */
export function LineChart({ points, fit, xLabel, yLabel, unit, height = 280 }) {
  const { ref, tip, show, hide } = useTip();
  const W = 520, H = height, padL = 48, padR = 16, padB = 44, padT = 20;
  const xMax = Math.max(...points.map((p) => p[0])) * 1.05;
  const yMax = Math.max(...points.map((p) => p[1])) * 1.15;
  const x = (v) => padL + (W - padL - padR) * (v / xMax);
  const y = (v) => padT + (H - padT - padB) * (1 - v / yMax);
  const path = points.map((p, i) => `${i ? "L" : "M"}${x(p[0])},${y(p[1])}`).join(" ");

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage">
        {[0.25, 0.5, 0.75, 1].map((g) => (
          <g key={g}>
            <line x1={padL} x2={W - padR} y1={y(yMax * g / 1.15)} y2={y(yMax * g / 1.15)}
              stroke="var(--hairline-soft)" strokeWidth="1" />
            <text x={padL - 8} y={y(yMax * g / 1.15) + 3} fill="var(--muted)" fontSize="10"
              fontFamily={MONO} textAnchor="end">{Math.round(yMax * g / 1.15)}</text>
          </g>
        ))}
        <line x1={padL} x2={W - padR} y1={y(0)} y2={y(0)} stroke="var(--hairline)" strokeWidth="1" />
        {[0, 250, 500, 750, 1000].map((v) => (
          <text key={v} x={x(v)} y={H - padB + 16} fill="var(--muted)" fontSize="10"
            fontFamily={MONO} textAnchor="middle">{v}</text>
        ))}
        {fit && (
          <line x1={x(0)} y1={y(fit.b)} x2={x(xMax)} y2={y(fit.a * xMax + fit.b)}
            stroke="var(--muted)" strokeWidth="1.2" strokeDasharray="4 4" />
        )}
        <path d={path} fill="none" stroke="var(--blue)" strokeWidth="2" strokeLinejoin="round" />
        {points.map((p) => (
          <circle key={p[0]} cx={x(p[0])} cy={y(p[1])} r="4.5" fill="var(--surface)"
            stroke="var(--blue)" strokeWidth="1.8"
            onMouseMove={(e) => show(e, <><span className="k">{p[0]}k requests&nbsp;</span>{p[1]} {unit}</>)}
            onMouseLeave={hide}
          />
        ))}
        <text x={W / 2 + 14} y={H - 6} fill="var(--muted)" fontSize="10.5" fontFamily={MONO} textAnchor="middle">
          {xLabel}
        </text>
        <text x={12} y={padT - 6} fill="var(--muted)" fontSize="10.5" fontFamily={MONO}>{yLabel}</text>
        {fit && (
          <text x={x(560)} y={y(fit.a * 560 + fit.b) + 26} fill="var(--ink-2)" fontSize="10.5" fontFamily={MONO}>
            linear: ≈{fit.a} ms / 1k requests
          </text>
        )}
      </svg>
      <TipBox tip={tip} />
    </div>
  );
}

/* ---------- overlap-vs-stack horizontal bars ---------- */
export function OverlapChart({ title, deliv, durRide, durStack, xmax, unit, rideNote, stackNote }) {
  const { ref, tip, show, hide } = useTip();
  const W = 520, H = 190, padL = 96, padR = 14, padT = 34, rowH = 30;
  const x = (v) => padL + (W - padL - padR) * (v / xmax);
  const rows = [
    { y: padT + 18, label: "overlap (in-fabric)" },
    { y: padT + 18 + 62, label: "stack (serial)" },
  ];

  return (
    <div ref={ref} style={{ position: "relative" }}>
      <svg viewBox={`0 0 ${W} ${H}`} className="svg-stage">
        <text x={padL} y={16} fill="var(--ink-2)" fontSize="11.5" fontFamily={MONO}>{title}</text>
        {rows.map((r) => (
          <text key={r.label} x={padL - 10} y={r.y + rowH / 2 + 4} fill="var(--muted)" fontSize="10.5"
            fontFamily={MONO} textAnchor="end">
            {r.label.split(" ").map((s, j) => (
              <tspan key={j} x={padL - 10} dy={j === 0 ? -5 : 12}>{s}</tspan>
            ))}
          </text>
        ))}
        {/* row 1: barrier with durable write riding under it */}
        <rect x={x(0)} y={rows[0].y} width={x(deliv) - x(0)} height={rowH} rx="4"
          fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.2"
          onMouseMove={(e) => show(e, <><span className="k">reliable-delivery barrier&nbsp;</span>{fmt(deliv)} {unit}</>)}
          onMouseLeave={hide} />
        <rect x={Math.max(x(deliv * 0.8), x(0) + 2)} y={rows[0].y + rowH - 12}
          width={Math.max(x(durRide) - x(0), 5)} height="8" rx="3"
          fill="var(--green)"
          onMouseMove={(e) => show(e, <><span className="k">durable write (rides)&nbsp;</span>{durRide} {unit}</>)}
          onMouseLeave={hide} />
        <text x={x(deliv) + 8} y={rows[0].y + rowH / 2 + 4} fill="var(--green)" fontSize="10.5" fontFamily={MONO}>
          {rideNote}
        </text>
        {/* row 2: barrier then serial write stacked */}
        <rect x={x(0)} y={rows[1].y} width={x(deliv) - x(0)} height={rowH} rx="4"
          fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.2"
          onMouseMove={(e) => show(e, <><span className="k">reliable-delivery barrier&nbsp;</span>{fmt(deliv)} {unit}</>)}
          onMouseLeave={hide} />
        <rect x={x(deliv)} y={rows[1].y} width={x(deliv + durStack) - x(deliv)} height={rowH} rx="4"
          fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.2"
          onMouseMove={(e) => show(e, <><span className="k">durable write (stacks)&nbsp;</span>{fmt(durStack)} {unit}</>)}
          onMouseLeave={hide} />
        <text x={Math.min(x(deliv + durStack) + 8, W - 90)} y={rows[1].y + rowH / 2 + 4}
          fill="var(--red)" fontSize="10.5" fontFamily={MONO}>
          {stackNote}
        </text>
        {/* axis */}
        <line x1={padL} x2={W - padR} y1={H - 26} y2={H - 26} stroke="var(--hairline)" strokeWidth="1" />
        {[0, 0.5, 1].map((g) => (
          <text key={g} x={x(xmax * g)} y={H - 10} fill="var(--muted)" fontSize="10" fontFamily={MONO}
            textAnchor={g === 0 ? "start" : g === 1 ? "end" : "middle"}>
            {fmt(Math.round(xmax * g))} {g === 1 ? unit : ""}
          </text>
        ))}
      </svg>
      <TipBox tip={tip} />
    </div>
  );
}
