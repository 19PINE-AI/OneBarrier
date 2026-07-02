import { Section, Reveal } from "./ui";
import { BarChartV } from "./charts";

/* The determinism boundary: wall clock desyncs, virtual clock replays */
function BoundaryDiagram() {
  const inputs = [120, 250, 420, 560]; // input-event x positions
  const timerRec = [90, 175, 260, 345, 430, 515, 600];
  const timerRep = [90, 200, 310, 420, 530, 640];
  const Row = ({ y, label, ticks, tickColor, tickShape, verdict }) => (
    <g>
      <text x="24" y={y - 16} fill="var(--ink-2)" fontSize="11.5" fontFamily="var(--font-mono)">{label}</text>
      <line x1="24" y1={y} x2="660" y2={y} stroke="var(--hairline)" strokeWidth="1" />
      {inputs.map((x) => (
        <g key={x}>
          <circle cx={x} cy={y} r="6" fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.2" />
        </g>
      ))}
      {ticks.map((x, i) =>
        tickShape === "tri" ? (
          <polygon key={i} points={`${x},${y - 12} ${x - 5},${y - 3} ${x + 5},${y - 3}`}
            fill="none" stroke={tickColor} strokeWidth="1.2" />
        ) : (
          <line key={i} x1={x} y1={y - 11} x2={x} y2={y - 3} stroke={tickColor} strokeWidth="1.4" />
        )
      )}
      <text x="690" y={y + 4} fill={verdict.color} fontSize="12" fontFamily="var(--font-mono)">{verdict.text}</text>
    </g>
  );
  return (
    <svg viewBox="0 0 920 360" className="svg-stage" role="img"
      aria-label="The determinism boundary: under wall-clock time a background timer fires a different number of times between record and replay; the virtual clock ticks once per input event so both runs read identical time.">
      <text x="24" y="34" fill="var(--red)" fontSize="12.5" fontFamily="var(--font-mono)" fontWeight="600">
        wall clock — timer-driven reads desync
      </text>
      <Row y={80} label="record   (serverCron fires 7×)" ticks={timerRec} tickColor="var(--red)" tickShape="tri"
        verdict={{ color: "var(--red)", text: "" }} />
      <Row y={140} label="replay   (fires 6× — state diverges)" ticks={timerRep} tickColor="var(--red)" tickShape="tri"
        verdict={{ color: "var(--red)", text: "✕ differs" }} />

      <text x="24" y="212" fill="var(--green)" fontSize="12.5" fontFamily="var(--font-mono)" fontWeight="600">
        virtual clock — one tick per input event
      </text>
      <Row y={258} label="record   (t = base + Σ deltas)" ticks={inputs.map((x) => x)} tickColor="var(--green)" tickShape="bar"
        verdict={{ color: "var(--green)", text: "" }} />
      <Row y={318} label="replay   (same inputs → same time)" ticks={inputs.map((x) => x)} tickColor="var(--green)" tickShape="bar"
        verdict={{ color: "var(--green)", text: "✓ identical" }} />
      <text x="342" y="352" fill="var(--muted)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
        ● input events (supplied in order by the fabric) — time advances by the logged real inter-arrival delta
      </text>
    </svg>
  );
}

const SHARD_DATA = [
  { label: "-t1", value: 342, color: "var(--muted)" },
  { label: "-t2", value: 575, color: "var(--muted)" },
  { label: "-t4", value: 821, color: "var(--blue)" },
  { label: "-t8", value: 1239, color: "var(--muted)" },
  { label: "-t1 +libOS", value: 302, color: "var(--yellow)" },
  { label: "4× -t1 shards", value: 1008, color: "var(--green)" },
];

export default function Libos() {
  return (
    <Section
      id="libos"
      ts="004"
      kicker="§4 · The determinism libOS"
      title={
        <>
          Once order comes from the fabric, the residual
          non-determinism is <em>local</em> — and closable.
        </>
      }
    >
      <p className="lede">
        Three composable <span className="mono" style={{ fontSize: "0.92em" }}>LD_PRELOAD</span>{" "}
        libraries, ~900 lines of C, no kernel module, no application change. They close what
        remains after message order is external: the <strong>clock</strong>, the{" "}
        <strong>random source</strong>, and <strong>thread interleaving</strong>. Given the same
        input sequence, execution becomes byte-identical — the HTTP{" "}
        <span className="mono" style={{ fontSize: "0.92em" }}>Date</span> header Nginx formats
        from its cached time matches across a crash and a multi-second gap.
      </p>

      <Reveal>
        <div className="panel" style={{ marginTop: 34 }}>
          <div className="panel-head"><span className="title">the determinism boundary — why a virtual clock</span></div>
          <div style={{ padding: "18px 14px 6px" }}>
            <BoundaryDiagram />
          </div>
          <div className="panel-caption">
            <b>Request-driven</b> time reads (a timestamp stamped into a reply) replay fine by
            position. <b>Timer-driven</b> reads (Redis <span className="mono">serverCron</span>,
            Nginx’s cached-time update, Memcached’s LRU maintainer) fire a <b>different number of
            times</b> between record and replay — desynchronizing any position-indexed scheme.
            The virtual clock makes time a function of the <b>input prefix</b>, not of read count:
            wall-clock-faithful <i>and</i> byte-identical.
          </div>
        </div>
      </Reveal>

      <div className="two-col" style={{ marginTop: 18 }}>
        <Reveal>
          <div className="panel collapse-card" style={{ height: "100%" }}>
            <div className="eq">randomness · trapped at the syscall</div>
            <h3>The boundary is per-consumer, not per-interface</h3>
            <p style={{ marginBottom: 10 }}>
              Real servers defeat library interposition: V8, OpenSSL and{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>arc4random</span> issue the raw{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>getrandom</span> syscall, and Redis reads{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>/dev/urandom</span> through{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>fopen</span>. So the libOS traps{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>getrandom</span> with seccomp-BPF and redirects{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>/dev/urandom</span> via a private mount
              namespace to a deterministic stream.
            </p>
            <p>
              One instructive hole: V8’s <span className="mono" style={{ fontSize: "0.92em" }}>Math.random</span>{" "}
              seeds from the <span className="mono" style={{ fontSize: "0.92em" }}>RDRAND</span> instruction —
              no syscall, no symbol — pinned instead with a recorded{" "}
              <span className="mono" style={{ fontSize: "0.92em" }}>--random-seed</span> launch flag.
            </p>
          </div>
        </Reveal>
        <Reveal delay={0.1}>
          <div className="panel">
            <div className="panel-head"><span className="title">threads · share-nothing shards beat -t 4 (memcached, k ops/s)</span></div>
            <div className="chart-body">
              <BarChartV data={SHARD_DATA} unit="k ops/s" refLine={{ value: 821, label: "-t4 native" }} />
            </div>
            <div className="panel-caption">
              A Kendo-style deterministic scheduler is correct but serializes critical sections
              (3.2× slower on a lock microbench, &gt;1000× on a contended server). The performant
              deterministic path is what high-performance servers already do:{" "}
              <b>N single-thread shards</b>, each deterministic by construction — four shards
              out-throughput one four-worker process.
            </div>
          </div>
        </Reveal>
      </div>
    </Section>
  );
}
