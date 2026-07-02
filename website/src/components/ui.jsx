import { motion } from "motion/react";

export function Section({ id, ts, kicker, title, children }) {
  return (
    <section className="block" id={id}>
      <div className="wrap">
        <motion.div
          initial={{ opacity: 0, y: 24 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, margin: "-80px" }}
          transition={{ duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
        >
          <div className="section-rule">
            <span className="ts-chip">ts={ts}</span>
            <span className="overline">{kicker}</span>
          </div>
          <h2 className="section-title">{title}</h2>
          {children}
        </motion.div>
      </div>
    </section>
  );
}

export function Reveal({ children, delay = 0 }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 22 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true, margin: "-60px" }}
      transition={{ duration: 0.6, delay, ease: [0.22, 1, 0.36, 1] }}
    >
      {children}
    </motion.div>
  );
}

/** Panel header with a play/pause control */
export function PlayerHead({ title, playing, setPlaying, extra }) {
  return (
    <div className="panel-head">
      <span className="title">{title}</span>
      <div className="player">
        {extra}
        <button className="primary" onClick={() => setPlaying(!playing)}>
          {playing ? "❚❚ pause" : "▶ play"}
        </button>
      </div>
    </div>
  );
}

/** Stage chips linked to a loop timeline */
export function StageChips({ stages, t, seek }) {
  const active = stages.findIndex(
    (s, i) => t >= s.at && (i === stages.length - 1 || t < stages[i + 1].at)
  );
  return (
    <div className="stage-chips">
      {stages.map((s, i) => (
        <button
          key={s.label}
          className={i === active ? "active" : ""}
          onClick={() => seek(s.at + 0.001)}
        >
          {i + 1} · {s.label}
        </button>
      ))}
    </div>
  );
}
