import { useEffect, useRef, useState } from "react";
import { on, type Status } from "./api";
import "./Overlay.css";

const BARS = 21;
const MAX_HEIGHT = 30; // px, matches .bars span height in Overlay.css
const CENTER = (BARS - 1) / 2;

// Fixed per-bar speed and phase so each bar wobbles differently, like a live waveform.
const BAR_MOTION = Array.from({ length: BARS }, (_, i) => ({
  speed: 5 + ((i * 7) % 5) * 1.3,
  phase: (i * 2.39996) % (Math.PI * 2),
  // Bell curve: middle bars react most, like a voice spectrum.
  envelope: Math.exp(-(((i - CENTER) / (CENTER * 0.6)) ** 2)),
}));

/**
 * The floating "Listening…" pill. Bars follow the microphone loudness, smoothed at
 * 60 fps; when it's quiet they ripple gently so it's clear Dictate is still listening.
 */
export default function Overlay() {
  const [status, setStatus] = useState<Status>("recording");
  const bars = useRef<(HTMLSpanElement | null)[]>([]);
  const dot = useRef<HTMLSpanElement>(null);
  const target = useRef(0);
  const statusRef = useRef(status);
  statusRef.current = status;

  useEffect(() => {
    const subs = [
      on.status((s) => {
        setStatus(s);
        if (s !== "recording") target.current = 0;
      }),
      on.level((l) => (target.current = l)),
    ];
    return () => subs.forEach((p) => p.then((unlisten) => unlisten()));
  }, []);

  useEffect(() => {
    let level = 0;
    let frame = 0;
    const tick = (now: number) => {
      const t = now / 1000;
      // Attack fast, release slowly, so speech looks lively but not jittery.
      const k = target.current > level ? 0.45 : 0.1;
      level += (target.current - level) * k;

      const recording = statusRef.current === "recording";
      bars.current.forEach((bar, i) => {
        if (!bar) return;
        const { speed, phase, envelope } = BAR_MOTION[i];
        let h: number;
        if (recording) {
          const wobble = 0.55 + 0.45 * Math.sin(t * speed + phase);
          const idle = 0.12 + 0.06 * Math.sin(t * 3 - i * 0.45); // gentle ripple in silence
          h = Math.max(idle, level * envelope * wobble);
        } else {
          // Transcribing: a travelling wave.
          h = 0.15 + 0.35 * (0.5 + 0.5 * Math.sin(t * 7 - i * 0.55));
        }
        bar.style.transform = `scaleY(${Math.min(1, h)})`;
      });

      if (dot.current) {
        const pulse = recording ? level : 0;
        dot.current.style.transform = `scale(${1 + pulse * 0.7})`;
        dot.current.style.boxShadow = `0 0 ${4 + pulse * 14}px ${pulse * 5}px rgb(239 68 68 / ${0.25 + pulse * 0.5})`;
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, []);

  const recording = status === "recording";
  return (
    <div className={`pill ${recording ? "" : "busy"}`}>
      <span className="mic" ref={dot} />
      <div className="bars" style={{ height: MAX_HEIGHT }}>
        {Array.from({ length: BARS }, (_, i) => (
          <span key={i} ref={(el) => void (bars.current[i] = el)} />
        ))}
      </div>
      <span className="label">{recording ? "Listening" : "Transcribing"}</span>
    </div>
  );
}
