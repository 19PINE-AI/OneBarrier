import { Section, Reveal } from "./ui";

/* positioning quadrant: operating point × approach */
function Positioning() {
  const pts = [
    { x: 200, y: 96, label: "Remus / VMware FT", c: "var(--muted)" },
    { x: 152, y: 130, label: "LLFT", c: "var(--muted)" },
    { x: 258, y: 138, label: "HyCoR", c: "var(--muted)" },
    { x: 236, y: 268, label: "Temporal / Flink", c: "var(--muted)" },
    { x: 636, y: 250, label: "NOPaxos / Eris (active SMR)", c: "var(--blue)" },
  ];
  return (
    <svg viewBox="0 0 920 340" className="svg-stage" role="img"
      aria-label="Positioning chart: prior transparent systems live at the millisecond software-ordered operating point; rewrite paths abandon transparency; network-ordered SMR is not transparent; OneBarrier occupies the transparent, microsecond, in-network-order corner.">
      {/* axes */}
      <line x1="70" y1="300" x2="880" y2="300" stroke="var(--muted)" strokeWidth="1" />
      <line x1="70" y1="300" x2="70" y2="30" stroke="var(--muted)" strokeWidth="1" />
      <text x="880" y="322" fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="end">
        operating point → in-network order, µs RTT
      </text>
      <text x="70" y="322" fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)">
        software order, ms
      </text>
      <text x="60" y="36" fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)" textAnchor="end"
        transform="rotate(-90 60 36)" />
      <text x="84" y="46" fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)">↑ transparent (unmodified binaries)</text>
      <text x="84" y="290" fill="var(--muted)" fontSize="11" fontFamily="var(--font-mono)">↓ rewrite the application</text>

      {/* mid hairlines */}
      <line x1="70" y1="180" x2="880" y2="180" stroke="var(--hairline-soft)" strokeWidth="1" />
      <line x1="450" y1="30" x2="450" y2="300" stroke="var(--hairline-soft)" strokeWidth="1" />

      {pts.map((p) => (
        <g key={p.label}>
          <circle cx={p.x} cy={p.y} r="7" fill="transparent" stroke={p.c} strokeWidth="1.6" />
          <text x={p.x + 14} y={p.y + 4} fill="var(--ink-2)" fontSize="11.5" fontFamily="var(--font-mono)">{p.label}</text>
        </g>
      ))}

      {/* OneBarrier */}
      <g>
        <circle cx="742" cy="86" r="11" fill="var(--amber-dim)" stroke="var(--amber)" strokeWidth="2" />
        <circle cx="742" cy="86" r="20" fill="none" stroke="var(--amber)" strokeWidth="1" opacity="0.4" />
        <text x="742" y="56" fill="var(--amber)" fontSize="13" fontFamily="var(--font-mono)" textAnchor="middle" fontWeight="600">
          OneBarrier
        </text>
        <text x="742" y="118" fill="var(--ink-2)" fontSize="10.5" fontFamily="var(--font-mono)" textAnchor="middle">
          transparent · passive · µs
        </text>
      </g>
    </svg>
  );
}

const PRIOR = [
  {
    sys: "LLFT",
    did: "Transparent FT of unmodified socket apps with no order-log, via totally-ordered virtual time.",
    gap: "Its order source is host software. OneBarrier offloads order to the switch and shows the cost regime changes.",
  },
  {
    sys: "HyCoR",
    did: "Checkpoint-plus-replay FT of unmodified containers.",
    gap: "Still logs non-determinism over a normal network. The fabric removes the order-log entirely.",
  },
  {
    sys: "NOPaxos · Eris · Derecho",
    did: "Network-offloaded ordering for consensus and SMR.",
    gap: "Active SMR — N replicas execute. OneBarrier is passive: 1 live + log + snapshot, ~1× execution CPU.",
  },
  {
    sys: "Remus · VMware FT",
    did: "Transparent VM-level FT, output buffered until the next sync.",
    gap: "That output-hold latency is exactly what the barrier coincidence removes.",
  },
];

const LIMITS = [
  <><b>No RDMA/P4 hardware.</b> The overlap’s µs-scale magnitude comes from a calibrated discrete-event model plus real SoftRoCE verbs — the one major claim not measured on real artifacts.</>,
  <><b>In-memory durability.</b> f-of-k fail-stop tolerance via in-fabric replication, not crash-consistent persistence across correlated power loss (the FaRM/RAMCloud tradeoff).</>,
  <><b>No automated failover.</b> The recovery mechanism is validated; primary-promotion / view-change is left to a production layer. A primary failure costs a recovery window — the price of passive.</>,
  <><b>Shared-everything binaries</b> fall back to the checkpoint-only CRIU path (PostgreSQL, MariaDB) — cross-process shared-memory order is not replayed.</>,
  <><b>RDRAND / RDTSC.</b> Two userspace instructions take no syscall; RDRAND is pinned per-consumer, RDTSC is unused by the five servers but not yet generally trapped.</>,
  <><b>Non-cooperating peers — the deepest boundary, an impossibility.</b> Transparent FT cannot un-send an effect delivered to a peer that won’t cooperate. The clean wins are fabric-internal, idempotent, self-contained leaf services.</>,
];

export default function Reckoning() {
  return (
    <Section
      id="reckoning"
      ts="007"
      kicker="§7 · An honest reckoning"
      title={
        <>
          Not a new primitive — a realization-and-measurement,
          at the operating point where it finally matters.
        </>
      }
    >
      <p className="lede">
        An adversarial prior-art review finds the conceptual thesis substantially anticipated,
        and the paper says so plainly: the vanishing order-log is Schneider’s decades-old SMR
        property, and LLFT did transparent no-order-log FT in 2013. What survives:{" "}
        <strong>OneBarrier is the first system to show that a single in-network total-order
        reliable fabric makes transparent passive FT cost ≈ the total-order baseline</strong> —
        and to demonstrate the enabling determinism libOS on five real servers.
      </p>

      <Reveal>
        <div className="panel" style={{ marginTop: 34 }}>
          <div className="panel-head"><span className="title">where OneBarrier sits</span></div>
          <div style={{ padding: "16px 10px 6px" }}>
            <Positioning />
          </div>
        </div>
      </Reveal>

      <Reveal>
        <div className="panel" style={{ marginTop: 18 }}>
          <div className="panel-head"><span className="title">prior art, and the gap OneBarrier owns</span></div>
          <table className="prior-table">
            <thead>
              <tr><th>prior</th><th>what it already did</th><th>the gap</th></tr>
            </thead>
            <tbody>
              {PRIOR.map((p) => (
                <tr key={p.sys}>
                  <td>{p.sys}</td>
                  <td>{p.did}</td>
                  <td><span className="own">{p.gap}</span></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Reveal>

      <h3 style={{ marginTop: 48, fontSize: "1.5rem" }}>Limitations, stated up front</h3>
      <ul className="limits">
        {LIMITS.map((l, i) => (
          <li key={i}>{l}</li>
        ))}
      </ul>
    </Section>
  );
}
