import { Section, Reveal } from "./ui";
import { OverlapChart, LineChart, GroupedBars } from "./charts";

const MATRIX = [
  { app: "redis", time: "✓", rng: "✓", state: "✓ TTL expiries, SPOP order", path: "deterministic replay" },
  { app: "memcached", time: "✓", rng: "✓", state: "✓ LRU eviction order", path: "deterministic replay" },
  { app: "nginx", time: "✓", rng: "✓", state: "✓ Date header, byte-identical", path: "deterministic replay" },
  { app: "node.js", time: "✓", rng: "✓", state: "✓ Math.random session IDs", path: "deterministic replay" },
  { app: "postgresql", time: "—", rng: "—", state: "✓ multi-process on-disk tree", path: "CRIU checkpoint" },
];

const FIT = [
  { q: "deterministic given input order?", a: "the libOS closes time, randomness, and scheduling — the fabric closes order" },
  { q: "share-nothing, or shardable?", a: "single-thread shards replay; shared-everything binaries take the CRIU path" },
  { q: "socket-based I/O?", a: "the LD_PRELOAD shim routes the POSIX socket surface through the fabric" },
  { q: "bounded output per request?", a: "suppression needs a sequence point to key each externalized effect" },
];

export default function Results() {
  return (
    <Section
      id="results"
      ts="006"
      kicker="§6 · Measured, on stock binaries"
      title={
        <>
          The coincidence, quantified: durability rides the
          barrier at <em>0.23%</em> marginal cost.
        </>
      }
    >
      <p className="lede">
        The experimental question is binary: does the durable write <strong>ride</strong> the
        barrier the fabric already crosses, or <strong>stack</strong> on top of it? Measured on
        the loopback reproduction and projected at 1Pipe’s published RDMA operating point, the
        answer is the same structure — and the serial-fsync tier reproduces exactly the
        output-hold failure mode that sank Remus (commit latency 2×, throughput collapse).
      </p>

      <div className="charts-grid">
        <Reveal>
          <div className="panel">
            <div className="panel-head"><span className="title">ride vs. stack — measured (loopback)</span></div>
            <div className="chart-body">
              <OverlapChart
                title="marginal durable write on the critical path"
                deliv={2014} durRide={4.59} durStack={2963} xmax={5400} unit="µs"
                rideNote="+4.59 µs · +0.23%" stackNote="+2,963 µs" />
            </div>
            <div className="panel-caption">
              Delivery p50 is 2,014&thinsp;µs; adding in-fabric durability moves commit to
              2,018&thinsp;µs. Serial fsync moves it to 6,016&thinsp;µs — commit doubles and the
              executor saturates near 340&thinsp;ops/s, the knee the calibrated simulator predicted.
            </div>
          </div>
        </Reveal>
        <Reveal delay={0.08}>
          <div className="panel">
            <div className="panel-head"><span className="title">ride vs. stack — RDMA operating point (model)</span></div>
            <div className="chart-body">
              <OverlapChart
                title="same structure at 1Pipe’s parameters"
                deliv={21} durRide={1.5} durStack={100} xmax={158} unit="µs"
                rideNote="≈ +0" stackNote="+100 µs" />
            </div>
            <div className="panel-caption">
              A ~1.5&thinsp;µs replica write completes inside the ~21&thinsp;µs barrier; serial
              durability stacks ~100&thinsp;µs. In the load sweep, the FT tier’s p99.9 tail is{" "}
              <b>coincident with the no-FT baseline at every load</b> up to the apply-bound knee.
              (Honest caveat: model + SoftRoCE verbs, not a P4/RDMA testbed.)
            </div>
          </div>
        </Reveal>
        <Reveal>
          <div className="panel">
            <div className="panel-head"><span className="title">recovery time vs. replayed log (redis)</span></div>
            <div className="chart-body">
              <LineChart
                points={[[10, 35], [50, 59], [100, 87], [200, 146], [500, 269], [1000, 536]]}
                fit={{ a: 0.5, b: 30 }}
                xLabel="captured request log (×1,000 requests)"
                yLabel="ms" unit="ms" />
            </div>
            <div className="panel-caption">
              Affine: a ~30&thinsp;ms restore floor plus ~0.5&thinsp;ms per 1,000 requests
              (~95&thinsp;MB/s), with <b>exact</b> reconstruction. A checkpoint every 100k
              requests bounds downtime under 100&thinsp;ms — versus seconds-to-minutes for
              detect-and-restart cluster failover.
            </div>
          </div>
        </Reveal>
        <Reveal delay={0.08}>
          <div className="panel">
            <div className="panel-head"><span className="title">passive vs. active SMR — execution CPU (ms)</span></div>
            <div className="chart-body">
              <GroupedBars
                groups={[2, 3, 5, 7]}
                series={[
                  { name: "active SMR (N execute)", color: "var(--red)", values: [205.9, 309.5, 519.5, 729.7] },
                  { name: "OneBarrier (passive)", color: "var(--green)", values: [105.7, 108.7, 114.5, 123.1] },
                ]}
                notes={["−49%", "−65%", "−78%", "−83%"]}
                unit="ms" xLabel="replicas" />
            </div>
            <div className="panel-caption">
              Log-only backups never execute the state machine, so execution CPU stays near a
              single replica’s as the replication factor grows — where active SMR climbs
              linearly. Against HyCoR’s order-log, OneBarrier persists strictly fewer durable
              bytes and runs 1.5–2.2× faster.
            </div>
          </div>
        </Reveal>
      </div>

      <Reveal>
        <div className="panel" style={{ marginTop: 18 }}>
          <div className="panel-head">
            <span className="title">byte-identical recovery — five unmodified servers, kill −9 + real wall-clock gap</span>
          </div>
          <table className="matrix-table">
            <thead>
              <tr>
                <th>server</th><th>time</th><th>randomness</th><th>full state recovered</th><th>path</th>
              </tr>
            </thead>
            <tbody>
              {MATRIX.map((r) => (
                <tr key={r.app}>
                  <td className="app">{r.app}</td>
                  <td className={r.time === "✓" ? "yes" : "path"}>{r.time}</td>
                  <td className={r.rng === "✓" ? "yes" : "path"}>{r.rng}</td>
                  <td className="yes">{r.state}</td>
                  <td className="path">{r.path}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="panel-caption">
            Every row passes on the strongest reading — recovered state equals live state,
            byte for byte, while a <b>no-libOS control diverges in every trial</b> (25/25 redis,
            8/8 node.js). Determinism here is manufactured, not assumed. Ten further
            applications across five classes (brokers, databases, an NF, a microservice,
            infrastructure daemons) recover along the path the fit-test assigns — including a
            stock Redis stream broker with byte-identical entry IDs, and SQLite by{" "}
            <b>deterministic replay</b>, showing the database boundary is shared-everything
            concurrency, not “databases.”
          </div>
        </div>
      </Reveal>

      <div className="fit-strip">
        {FIT.map((f) => (
          <div className="fit-cell" key={f.q}>
            <div className="q">{f.q}</div>
            <p>{f.a}</p>
          </div>
        ))}
      </div>
      <p className="small" style={{ marginTop: 12 }}>
        The fit-test: an unmodified server extends transparently iff it meets these four
        preconditions — then it routes to order-log-free replay (share-nothing socket servers)
        or general CRIU checkpointing (any shared-everything binary).
      </p>
    </Section>
  );
}
