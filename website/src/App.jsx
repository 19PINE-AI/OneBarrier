import Hero from "./components/Hero";
import Problem from "./components/Problem";
import OnePipe from "./components/OnePipe";
import Barrier from "./components/Barrier";
import Libos from "./components/Libos";
import Recovery from "./components/Recovery";
import Results from "./components/Results";
import Reckoning from "./components/Reckoning";

const LINKS = [
  ["#problem", "problem"],
  ["#fabric", "one pipe"],
  ["#onebarrier", "one barrier"],
  ["#libos", "libOS"],
  ["#recovery", "recovery"],
  ["#results", "results"],
  ["#reckoning", "reckoning"],
];

function Nav() {
  return (
    <nav className="nav">
      <div className="nav-inner">
        <a href="#top" className="nav-brand" style={{ textDecoration: "none" }}>
          one<span className="bar">|</span>barrier
        </a>
        <div className="nav-links">
          {LINKS.map(([href, label]) => (
            <a key={href} href={href}>{label}</a>
          ))}
        </div>
      </div>
    </nav>
  );
}

function Conclusion() {
  return (
    <section className="block" id="conclusion" style={{ paddingBottom: 0 }}>
      <div className="wrap">
        <div className="section-rule">
          <span className="ts-chip">ts=008</span>
          <span className="overline">§8 · Conclusion</span>
        </div>
        <h2 className="section-title" style={{ maxWidth: 900 }}>
          Transparent fault tolerance failed because three coupled costs were large at the
          millisecond operating point — not because snapshots are hard. At the microsecond
          operating point, it becomes <em style={{ color: "var(--amber)" }}>a byproduct of the
          communication fabric.</em>
        </h2>
        <p className="lede">
          A determinism libOS makes unmodified Redis, Memcached, Nginx, Node.js, and PostgreSQL
          recoverable at a few-percent overhead. An in-network total-order reliable fabric
          collapses order logging, the distributed cut, and output commit into mechanisms it
          already runs for communication correctness. One pipe; one barrier; fault tolerance
          for free.
        </p>
      </div>
    </section>
  );
}

function Footer() {
  return (
    <footer className="footer">
      <div className="wrap footer-inner">
        <div className="mono">
          <div style={{ color: "var(--ink)" }}>OneBarrier: Transparent Fault Tolerance for Free</div>
          <div>Bojie Li · Pine AI</div>
          <div>
            <a href="https://github.com/bojieli/OneBarrier" target="_blank" rel="noreferrer">github.com/bojieli/OneBarrier</a>
          </div>
        </div>
        <div className="mono" style={{ textAlign: "right" }}>
          <div>every result reproduced by a single command</div>
          <div>protocols machine-checked in TLA+ · 3.5×10⁶ states</div>
          <div style={{ color: "var(--amber)" }}>this site’s animations are a deterministic function of t</div>
        </div>
      </div>
    </footer>
  );
}

export default function App() {
  return (
    <>
      <Nav />
      <Hero />
      <Problem />
      <OnePipe />
      <Barrier />
      <Libos />
      <Recovery />
      <Results />
      <Reckoning />
      <Conclusion />
      <Footer />
    </>
  );
}
