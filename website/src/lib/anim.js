import { useEffect, useRef, useState, useCallback } from "react";

/**
 * A looping animation clock: returns t in [0,1) advancing over `duration` ms.
 * Pauses automatically when the element is off-screen.
 */
export function useLoop(duration, ref, { autoplay = true } = {}) {
  const [t, setT] = useState(0);
  const [playing, setPlaying] = useState(autoplay);
  const visible = useRef(true);
  const tRef = useRef(0);

  useEffect(() => {
    if (!ref.current) return;
    const io = new IntersectionObserver(
      ([e]) => { visible.current = e.isIntersecting; },
      { threshold: 0.05 }
    );
    io.observe(ref.current);
    return () => io.disconnect();
  }, [ref]);

  useEffect(() => {
    if (!playing) return;
    let raf, last = performance.now();
    const step = (now) => {
      const dt = Math.min(now - last, 100);
      last = now;
      if (visible.current) {
        tRef.current = (tRef.current + dt / duration) % 1;
        setT(tRef.current);
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [playing, duration]);

  const seek = useCallback((v) => {
    tRef.current = ((v % 1) + 1) % 1;
    setT(tRef.current);
  }, []);

  return { t, playing, setPlaying, seek };
}

/** progress of t through [a,b], clamped to [0,1] */
export function seg(t, a, b) {
  if (t <= a) return 0;
  if (t >= b) return 1;
  return (t - a) / (b - a);
}

/** smooth ease-in-out */
export function ease(u) {
  return u * u * (3 - 2 * u);
}

/** eased segment */
export function eseg(t, a, b) {
  return ease(seg(t, a, b));
}

export function lerp(a, b, u) {
  return a + (b - a) * u;
}

/** pulse: 1 at u=mid decaying to 0 at edges of [a,b] */
export function pulse(t, a, b) {
  const u = seg(t, a, b);
  if (u <= 0 || u >= 1) return 0;
  return Math.sin(u * Math.PI);
}
