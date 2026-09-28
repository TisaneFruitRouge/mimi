import { useEffect, useId, useRef, useState } from "react";
import { type Transition, motion, useAnimate, useReducedMotion } from "motion/react";
import { cn } from "cn";

import { useAssistantName } from "@/lib/queries";

/**
 * The assistant as a small, soft character: a lime mochi with a sprout on top.
 *
 * Pure SVG. The slow, always-on motion (breathing, the sprout swaying) is CSS (see the
 * "Assistant avatar" block in index.css), so no script runs per frame; blinks, glances
 * and mood changes are a few timers and `motion` springs. Everything pauses while the
 * avatar is off screen or the window is hidden, and under reduced motion only the
 * blinks remain.
 *
 * The same drawing is the app icon (`src-tauri/icons/source/`); keep them in step.
 */
export type AvatarMood = "idle" | "thinking" | "happy" | "listening" | "sleepy";

type Point = { x: number; y: number };

const BODY = "M50 18C76 18 91 38 91 60C91 80 74 88 50 88C26 88 9 80 9 60C9 38 24 18 50 18Z";
const EYES_X = [37.5, 62.5];
const EYES_Y = 54.75;
const INK = { fill: "#1d1d1f", stroke: "#1d1d1f", strokeWidth: 1.2, strokeLinejoin: "round" } as const;

// Every eye shape has the same two curves, so they morph into each other. Drawn around
// the eye's centre.
const EYE = {
  open: "M -5 0 C -5 -9 5 -9 5 0 C 5 9 -5 9 -5 0 Z",
  wide: "M -5.8 0 C -5.8 -10.4 5.8 -10.4 5.8 0 C 5.8 10.4 -5.8 10.4 -5.8 0 Z",
  // ^ ^ : a thick arch.
  happy: "M -5.6 2.4 C -5.6 -8 5.6 -8 5.6 2.4 C 3.4 -3.4 -3.4 -3.4 -5.6 2.4 Z",
  // Nearly shut: a soft downward crescent.
  sleepy: "M -5.6 1.4 C -3 3.4 3 3.4 5.6 1.4 C 5.6 8.8 -5.6 8.8 -5.6 1.4 Z",
};

// Mouths share one curve too. Drawn around (50, 66).
const MOUTH = {
  smile: "M -3.8 0 Q 0 3.4 3.8 0",
  grin: "M -5 -0.6 Q 0 5.6 5 -0.6",
  hmm: "M -2.4 1.3 Q 0 1.1 2.6 0.2",
  soft: "M -3 0.2 Q 0 2.8 3 0.2",
  tiny: "M -2.4 0.8 Q 0 2.4 2.4 0.8",
};

type Pose = {
  eye: keyof typeof EYE;
  mouth: keyof typeof MOUTH;
  /** Where the face looks, before glances. */
  face: Point;
  body: { rotate: number; scaleX: number; scaleY: number; y: number };
  /** The sprout's lean, in degrees. */
  leaf: number;
  blush: number;
  highlights: boolean;
  /** Seconds per breath. */
  breath: number;
  /** Seconds between blinks, [min, max]; null: no blinking. */
  blinks: [number, number] | null;
};

const POSES: Record<AvatarMood, Pose> = {
  idle: {
    eye: "open",
    mouth: "smile",
    face: { x: 0, y: 0 },
    body: { rotate: 0, scaleX: 1, scaleY: 1, y: 0 },
    leaf: 0,
    blush: 0.85,
    highlights: true,
    breath: 4.2,
    blinks: [2.4, 6],
  },
  thinking: {
    eye: "open",
    mouth: "hmm",
    face: { x: 2.2, y: -3 },
    body: { rotate: -4, scaleX: 1, scaleY: 1, y: 0 },
    leaf: 6,
    blush: 0.6,
    highlights: true,
    breath: 3.2,
    blinks: [2, 4.5],
  },
  happy: {
    eye: "happy",
    mouth: "grin",
    face: { x: 0, y: -1 },
    body: { rotate: 0, scaleX: 1, scaleY: 1, y: 0 },
    leaf: 0,
    blush: 1,
    highlights: false,
    breath: 3,
    blinks: null,
  },
  listening: {
    eye: "wide",
    mouth: "soft",
    face: { x: 0, y: 1.2 },
    body: { rotate: 0, scaleX: 1.035, scaleY: 1.035, y: 0.6 },
    leaf: -4,
    blush: 0.9,
    highlights: true,
    breath: 3.6,
    blinks: [3.5, 8],
  },
  sleepy: {
    eye: "sleepy",
    mouth: "tiny",
    face: { x: 0, y: 1.6 },
    body: { rotate: 2, scaleX: 1.025, scaleY: 0.965, y: 0 },
    leaf: -24,
    blush: 0.55,
    highlights: false,
    breath: 6,
    blinks: [5, 9],
  },
};

const SPRING = { type: "spring", stiffness: 420, damping: 38 } as const;
const SOFT_SPRING = { type: "spring", stiffness: 380, damping: 34 } as const;
const INSTANT = { duration: 0 } as const;

const between = ([min, max]: [number, number]) => (min + Math.random() * (max - min)) * 1000;
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function AssistantAvatar({
  size = 64,
  mood = "idle",
  decorative = false,
  label,
  className,
}: {
  /** Width and height in pixels. Below 40 px the face drops its small details. */
  size?: number;
  mood?: AvatarMood;
  /** Hidden from assistive technology when it only decorates something already named. */
  decorative?: boolean;
  /** Accessible name; defaults to the assistant's name. */
  label?: string;
  className?: string;
}) {
  const name = useAssistantName();
  const id = useSvgId();
  const reduced = useReducedMotion() ?? false;
  const pose = POSES[mood];
  const small = size < 40;
  const [scope, animate] = useAnimate<SVGSVGElement>();
  const awake = useAwake(scope);
  const [glance, setGlance] = useState<Point>({ x: 0, y: 0 });

  // Blinks at uneven intervals, sometimes twice in a row; slow, heavy ones when sleepy.
  useEffect(() => {
    if (!awake || !pose.blinks) return;
    const range = pose.blinks;
    const slow = mood === "sleepy";
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const blink = async () => {
      const times = !slow && Math.random() < 0.22 ? 2 : 1;
      for (let i = 0; i < times; i++) {
        if (stopped) return;
        await animate("[data-eyes]", { scaleY: 0.1 }, { duration: slow ? 0.28 : 0.09, ease: "easeIn" });
        if (slow) await wait(380);
        if (stopped) return;
        await animate("[data-eyes]", { scaleY: 1 }, { duration: slow ? 0.42 : 0.14, ease: "easeOut" });
      }
      if (!stopped) timer = setTimeout(blink, between(range));
    };
    timer = setTimeout(blink, between(range));
    return () => {
      stopped = true;
      clearTimeout(timer);
      if (scope.current) animate("[data-eyes]", { scaleY: 1 }, { duration: 0.12 });
    };
  }, [awake, mood, pose.blinks, animate, scope]);

  // Now and then a small look around; while thinking, the eyes wander up and aside.
  useEffect(() => {
    setGlance({ x: 0, y: 0 });
    if (!awake || reduced || mood === "happy" || mood === "sleepy") return;
    let timer: ReturnType<typeof setTimeout>;
    let side = 1;
    const look = () => {
      if (mood === "thinking") {
        side = -side;
        setGlance({ x: side > 0 ? 0 : -4, y: Math.random() * 0.8 });
        timer = setTimeout(look, between([1.6, 2.8]));
      } else if (Math.random() < 0.5) {
        const dir = Math.random() < 0.5 ? -1 : 1;
        setGlance({ x: dir * (1.6 + Math.random() * 1.2), y: Math.random() * 1.6 - 0.8 });
        timer = setTimeout(() => {
          setGlance({ x: 0, y: 0 });
          timer = setTimeout(look, between([5, 11]));
        }, between([1.1, 2.2]));
      } else {
        timer = setTimeout(look, between([5, 11]));
      }
    };
    timer = setTimeout(look, between(mood === "thinking" ? [0.8, 1.2] : [3, 7]));
    return () => clearTimeout(timer);
  }, [awake, mood, reduced]);

  // A little double hop when it becomes happy.
  const first = useRef(true);
  useEffect(() => {
    const initial = first.current;
    first.current = false;
    if (mood !== "happy" || reduced) return;
    const hop: Transition = {
      duration: 0.9,
      delay: initial ? 0.25 : 0,
      times: [0, 0.2, 0.42, 0.6, 0.78, 1],
      ease: "easeInOut",
    };
    animate(
      "[data-hop]",
      { y: [0, -7, 0, -3, 0, 0], scaleY: [1, 1.05, 0.94, 1.02, 0.98, 1], scaleX: [1, 0.97, 1.05, 0.99, 1.01, 1] },
      hop,
    );
    animate("[data-shadow-hop]", { scaleX: [1, 0.8, 1.04, 0.92, 1, 1], opacity: [1, 0.6, 1, 0.8, 1, 1] }, hop);
  }, [mood, reduced, animate]);

  const t = reduced ? INSTANT : SPRING;
  const soft = reduced ? INSTANT : SOFT_SPRING;
  const face = { x: pose.face.x + glance.x, y: pose.face.y + glance.y };

  return (
    <svg
      ref={scope}
      viewBox="0 0 100 100"
      width={size}
      height={size}
      className={cn("assistant-avatar shrink-0 overflow-visible select-none", !awake && "aa-paused", className)}
      style={{ ["--aa-breath" as string]: `${pose.breath}s` }}
      {...(decorative ? { "aria-hidden": true } : { role: "img", "aria-label": label ?? name })}
    >
      <defs>
        <BodyGradient id={`${id}-body`} />
        {/* Soft-edged cheeks: pink at the centre, fading into the lime. */}
        <radialGradient id={`${id}-blush`}>
          <stop offset="0.35" stopColor="#ff8aa5" stopOpacity={0.8} />
          <stop offset="1" stopColor="#ff8aa5" stopOpacity={0} />
        </radialGradient>
      </defs>

      {/* Ground shadow: shrinks as the body breathes in or hops. */}
      <g className="aa-shadow">
        <motion.ellipse data-shadow-hop cx="50" cy="91" rx="28" ry="3" fill="#000" fillOpacity={0.08} />
      </g>

      <motion.g initial={false} animate={pose.body} transition={soft} style={{ originX: 0.5, originY: 1 }}>
        <motion.g data-hop style={{ originX: 0.5, originY: 1 }}>
          <g className="aa-breath">
            <motion.g
              initial={false}
              animate={{ rotate: pose.leaf }}
              transition={soft}
              style={{ originX: 0.58, originY: 1 }}
            >
              <g className="aa-leaf">
                <Sprout />
              </g>
            </motion.g>

            <path d={BODY} fill={`url(#${id}-body)`} />
            {!small && (
              <ellipse cx="33" cy="30" rx="10.5" ry="5.2" fill="#fff" opacity={0.6} transform="rotate(-22 33 30)" />
            )}
            {!small && (
              <motion.g initial={false} animate={{ opacity: pose.blush }} transition={t}>
                <ellipse cx="24.5" cy="64" rx="8" ry="5" fill={`url(#${id}-blush)`} />
                <ellipse cx="75.5" cy="64" rx="8" ry="5" fill={`url(#${id}-blush)`} />
              </motion.g>
            )}

            {/* The face moves as one: eyes and mouth. */}
            <motion.g initial={false} animate={face} transition={soft}>
              <motion.g data-eyes style={{ originX: 0.5, originY: 0.5 }}>
                {EYES_X.map((cx) => (
                  <g key={cx} transform={`translate(${cx} ${EYES_Y})${small ? " scale(1.3)" : ""}`}>
                    <motion.path initial={false} animate={{ d: EYE[pose.eye] }} transition={t} {...INK} />
                    {!small && (
                      <motion.circle
                        initial={false}
                        cx="1.9"
                        cy="-3.2"
                        r="1.9"
                        fill="#fff"
                        animate={{ opacity: pose.highlights ? 1 : 0, scale: pose.highlights ? 1 : 0.2 }}
                        transition={t}
                      />
                    )}
                  </g>
                ))}
              </motion.g>
              {!small && (
                <g transform="translate(50 66)">
                  <motion.path
                    initial={false}
                    animate={{ d: MOUTH[pose.mouth] }}
                    transition={t}
                    fill="none"
                    stroke="#1d1d1f"
                    strokeWidth="2.3"
                    strokeLinecap="round"
                  />
                </g>
              )}
            </motion.g>
          </g>
        </motion.g>
      </motion.g>
    </svg>
  );
}

/**
 * The character standing still and simplified (no cheeks, shine or mouth; larger eyes),
 * so it stays crisp from 16 px up. For the logo mark and other small, static places.
 */
export function AssistantGlyph({ className }: { className?: string }) {
  const id = useSvgId();
  return (
    <svg viewBox="0 0 100 100" className={className} aria-hidden>
      <defs>
        <BodyGradient id={`${id}-body`} />
      </defs>
      <Sprout />
      <path d={BODY} fill={`url(#${id}-body)`} />
      {EYES_X.map((cx) => (
        <path key={cx} transform={`translate(${cx} ${EYES_Y}) scale(1.3)`} d={EYE.open} {...INK} />
      ))}
    </svg>
  );
}

function BodyGradient({ id }: { id: string }) {
  return (
    <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor="#e0f98f" />
      <stop offset="1" stopColor="#b4e140" />
    </linearGradient>
  );
}

/** The stem and its two leaves, growing from (50, 19.5) at the top of the head. */
function Sprout() {
  return (
    <>
      <path d="M50 19.5C50.4 14 49.4 10.5 47 7.5" fill="none" stroke="#6f9a1c" strokeWidth="2.6" strokeLinecap="round" />
      <path d="M47.6 9.2C43 3 35 3 32 6C35.5 11.5 42.5 12 47.6 9.2Z" fill="#9ccc2e" />
      <path d="M48.6 10.4C51.6 3.6 59.4 2.2 62.8 4.6C60.4 10.8 53.6 12.6 48.6 10.4Z" fill="#b8e24a" />
    </>
  );
}

/** A per-instance prefix for gradient ids, safe inside url(#…). */
function useSvgId() {
  return "aa" + useId().replace(/[^a-zA-Z0-9_-]/g, "");
}

/** True while the element is on screen and the window is visible. */
function useAwake(ref: React.RefObject<Element | null>) {
  const [onScreen, setOnScreen] = useState(true);
  const [visible, setVisible] = useState(() => typeof document === "undefined" || !document.hidden);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(([entry]) => setOnScreen(entry.isIntersecting));
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  useEffect(() => {
    const update = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  return onScreen && visible;
}
