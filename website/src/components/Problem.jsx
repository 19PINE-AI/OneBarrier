import { Section, Reveal } from "./ui";

function ArtOrderLog() {
  return (
    <svg viewBox="0 0 300 150" className="svg-stage">
      {/* two lanes of concurrent messages */}
      {[46, 78].map((y, i) => (
        <line key={y} x1="18" y1={y} x2="152" y2={y} stroke="var(--hairline)" strokeWidth="1" />
      ))}
      {[
        { y: 46, dur: "2.6s", ts: "5" },
        { y: 78, dur: "3.4s", ts: "3" },
      ].map((m) => (
        <g key={m.y}>
          <circle r="9" fill="var(--blue-dim)" stroke="var(--blue)" strokeWidth="1.2">
            <animate attributeName="cx" values="24;140" dur={m.dur} repeatCount="indefinite" />
            <animate attributeName="cy" values={`${m.y};${m.y}`} dur={m.dur} repeatCount="indefinite" />
          </circle>
        </g>
      ))}
      <text x="18" y="30" fill="var(--muted)" fontSize="10" fontFamily="var(--font-mono)">concurrent inputs</text>
      {/* the log tape */}
      <g>
        <rect x="176" y="38" width="106" height="74" rx="6" fill="var(--surface)" stroke="var(--hairline)" />
        <text x="229" y="30" fill="var(--muted)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">order log (every event)</text>
        {[0, 1, 2].map((r) => (
          <g key={r}>
            <rect x="184" y={46 + r * 20} width="90" height="14" rx="3" fill="var(--surface-3)">
              <animate attributeName="opacity" values="0.3;1;0.3" dur="3s" begin={`${r * 0.9}s`} repeatCount="indefinite" />
            </rect>
            <text x="190" y={57 + r * 20} fill="var(--ink-2)" fontSize="9" fontFamily="var(--font-mono)">
              {["recv B before A", "sched t2 < t1", "recv C before B"][r]}
            </text>
          </g>
        ))}
      </g>
    </svg>
  );
}

function ArtCut() {
  return (
    <svg viewBox="0 0 300 150" className="svg-stage">
      {[40, 75, 110].map((y, i) => (
        <g key={y}>
          <line x1="24" y1={y} x2="276" y2={y} stroke="var(--hairline)" strokeWidth="1" />
          <text x="10" y={y + 3} fill="var(--muted)" fontSize="9" fontFamily="var(--font-mono)">P{i + 1}</text>
        </g>
      ))}
      {/* zigzag marker cut */}
      <polyline
        points="150,26 150,58 190,58 190,92 132,92 132,124"
        fill="none" stroke="var(--yellow)" strokeWidth="1.6" strokeDasharray="5 4"
      >
        <animate attributeName="stroke-dashoffset" values="18;0" dur="1.6s" repeatCount="indefinite" />
      </polyline>
      <text x="150" y="16" fill="var(--yellow)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">marker cut</text>
      {/* in-flight message crossing the cut */}
      <line x1="118" y1="40" x2="212" y2="75" stroke="var(--red)" strokeWidth="1.3" strokeDasharray="3 3" />
      <circle r="6" fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.2">
        <animate attributeName="cx" values="118;212" dur="2.4s" repeatCount="indefinite" />
        <animate attributeName="cy" values="40;75" dur="2.4s" repeatCount="indefinite" />
      </circle>
      <text x="288" y="52" fill="var(--red)" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="end">in-flight state</text>
      <text x="288" y="64" fill="var(--red)" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="end">must be captured</text>
    </svg>
  );
}

function ArtOutputCommit() {
  return (
    <svg viewBox="0 0 300 150" className="svg-stage">
      {/* server */}
      <rect x="20" y="52" width="70" height="46" rx="7" fill="var(--surface)" stroke="var(--hairline)" />
      <text x="55" y="79" fill="var(--ink-2)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">server</text>
      {/* hold wall */}
      <line x1="176" y1="30" x2="176" y2="120" stroke="var(--red)" strokeWidth="2" />
      <text x="176" y="20" fill="var(--red)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">hold until durable</text>
      {/* the reply, held */}
      <g>
        <rect width="26" height="18" rx="3" fill="var(--red-dim)" stroke="var(--red)" strokeWidth="1.1" x="0" y="66">
          <animate attributeName="x" values="96;144;144;144;196;260" keyTimes="0;0.3;0.55;0.7;0.85;1" dur="4s" repeatCount="indefinite" />
        </rect>
      </g>
      <path d="M 96 75 h 60" stroke="var(--hairline)" strokeWidth="1" />
      <path d="M 196 75 h 84" stroke="var(--hairline)" strokeWidth="1" />
      <text x="100" y="108" fill="var(--muted)" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="middle">
        reply held for tens of ms
      </text>
      <text x="100" y="120" fill="var(--muted)" fontSize="9.5" fontFamily="var(--font-mono)" textAnchor="middle">
        (what sank Remus)
      </text>
      <text x="262" y="60" fill="var(--muted)" fontSize="10" fontFamily="var(--font-mono)" textAnchor="middle">client</text>
    </svg>
  );
}

const COSTS = [
  {
    num: "COST 01",
    title: "Logging non-determinism",
    art: <ArtOrderLog />,
    body: (
      <>
        To replay a failed replica to the exact state of the dead one, every non-deterministic
        event must be recorded — above all the <em>order</em> in which concurrent messages were
        processed. Order logging is the dominant per-event overhead of deterministic replay.
      </>
    ),
    kill: "killed by the fabric: the network is the order",
  },
  {
    num: "COST 02",
    title: "Coordinating a consistent cut",
    art: <ArtCut />,
    body: (
      <>
        A distributed snapshot must be globally consistent. The classical mechanism is
        Chandy–Lamport marker propagation, which coordinates all nodes and must capture
        the state of messages still in flight on every channel.
      </>
    ),
    kill: "killed by the fabric: a timestamp-T predicate, no markers",
  },
  {
    num: "COST 03",
    title: "Output commit",
    art: <ArtOutputCommit />,
    body: (
      <>
        Output that a client has seen cannot be un-sent on recovery, so every externalizing
        reply must be withheld until the state that produced it is durable. Remus paid this
        as tens of milliseconds of latency on every reply.
      </>
    ),
    kill: "killed by the fabric: the hold is a barrier it already crosses",
  },
];

export default function Problem() {
  return (
    <Section
      id="problem"
      ts="001"
      kicker="§1 · Why transparent FT never shipped"
      title={
        <>
          Three coupled costs, each a large fraction of latency
          at the <em>millisecond</em> operating point.
        </>
      }
    >
      <p className="lede">
        Making a server fault-tolerant without rewriting it is a four-decade goal that
        repeatedly failed to reach production. The reason was never the snapshot —
        checkpointing a process is well understood. It was three costs paid on the critical
        path of <strong>every request</strong>. Faced with them, industry chose the other path:
        rewrite applications to externalize state (Temporal, Flink, DBOS) — and left the legacy
        fleet behind.
      </p>
      <div className="costs">
        {COSTS.map((c, i) => (
          <Reveal key={c.num} delay={i * 0.12}>
            <div className="panel cost-card">
              <div className="art">{c.art}</div>
              <div className="body">
                <div className="num">{c.num}</div>
                <h3>{c.title}</h3>
                <p>{c.body}</p>
                <div className="kill">
                  <span className="x">✕</span>
                  <span>{c.kill}</span>
                </div>
              </div>
            </div>
          </Reveal>
        ))}
      </div>
      <div className="note-amber">
        <b>The operating-point argument.</b>&nbsp; The verdict against transparent FT is not
        wrong — it is <em>operating-point-dependent</em>. Route share-nothing servers through an
        in-network total-order reliable fabric with a 1–2&thinsp;µs RTT, and the three costs do
        not merely shrink. They <em>change structure</em>: each becomes a mechanism the fabric
        already runs for communication correctness alone.
      </div>
    </Section>
  );
}
