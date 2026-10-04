import { useId, useLayoutEffect, useRef, useState } from "react";
import type { AnimationEventHandler, Ref } from "react";
import { advanceMascotSequence, createMascotSequence, getCurrentMascotId } from "./mascotSequence";

import "./AttentionMascot.css";

export type MascotDanceId =
  | "01" | "02" | "03" | "04" | "05" | "06" | "07" | "08" | "09" | "10"
  | "11" | "12" | "13" | "14" | "15" | "16" | "17" | "18" | "19" | "20";

interface AttentionMascotProps {
  readonly variant?: MascotDanceId;
  readonly paused?: boolean;
}

interface MascotArtworkProps extends AttentionMascotProps {
  readonly stageRef?: Ref<HTMLDivElement>;
  readonly cycleRef?: Ref<SVGGElement>;
  readonly cycleKey?: number;
  readonly currentId?: MascotDanceId;
  readonly onAnimationEnd?: AnimationEventHandler<SVGGElement>;
}

/** Independently renderable artwork; explicit variants retain the seekable preview contract. */
export function AttentionMascot(props: AttentionMascotProps = {}) {
  return props.variant === undefined
    ? <MascotPlayback paused={props.paused} />
    : <AttentionMascotArtwork {...props} />;
}

function MascotPlayback({ paused = false }: AttentionMascotProps) {
  const [sequence, setSequence] = useState(() => createMascotSequence());
  const sequenceRef = useRef(sequence);
  const [userPaused, setUserPaused] = useState(false);
  const [gates, setGates] = useState(() => ({
    user: true,
    visible: document.visibilityState !== "hidden",
    onscreen: false,
    motion: !window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    external: !paused,
  }));
  const gatesRef = useRef(gates);
  const [cycle, setCycle] = useState(0);
  const stageRef = useRef<HTMLDivElement>(null);
  const cycleRef = useRef<SVGGElement>(null);
  const completed = useRef<SVGGElement | null>(null);

  function changeGate(gate: keyof typeof gates, open: boolean) {
    if (gatesRef.current[gate] === open) return;
    const next = { ...gatesRef.current, [gate]: open };
    gatesRef.current = next;
    setGates(next);
    if (open) setCycle((value) => value + 1);
  }

  useLayoutEffect(() => {
    changeGate("external", !paused);
  }, [paused]);

  useLayoutEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const visibility = () => changeGate("visible", document.visibilityState !== "hidden");
    const motion = () => changeGate("motion", !media.matches);
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.target === stageRef.current) changeGate("onscreen", entry.isIntersecting);
      }
    });
    document.addEventListener("visibilitychange", visibility);
    media.addEventListener("change", motion);
    visibility();
    motion();
    if (stageRef.current) observer.observe(stageRef.current);
    return () => {
      document.removeEventListener("visibilitychange", visibility);
      media.removeEventListener("change", motion);
      observer.disconnect();
    };
  }, []);

  const onAnimationEnd: AnimationEventHandler<SVGGElement> = (event) => {
    const current = gatesRef.current;
    if (event.target !== event.currentTarget
      || event.animationName !== "mascot-dance-cycle"
      || event.currentTarget !== cycleRef.current
      || completed.current === event.currentTarget
      || !current.user || !current.visible || !current.onscreen || !current.motion || !current.external) return;
    completed.current = event.currentTarget;
    const next = advanceMascotSequence(sequenceRef.current);
    sequenceRef.current = next;
    setSequence(next);
    setCycle((value) => value + 1);
  };
  const currentId = getCurrentMascotId(sequence);
  const playing = gates.user && gates.visible && gates.onscreen && gates.motion && gates.external;

  return (
    <div className="flex flex-col items-center">
      <AttentionMascotArtwork stageRef={stageRef} cycleRef={cycleRef} cycleKey={cycle}
        currentId={currentId} variant={gates.motion ? currentId : undefined}
        paused={!playing} onAnimationEnd={onAnimationEnd} />
      <button type="button" aria-label="Pause mascot animation" aria-pressed={userPaused}
        disabled={!gates.motion} tabIndex={gates.motion ? 0 : -1}
        style={{ visibility: gates.motion ? "visible" : "hidden" }}
        className="h-6 w-16 rounded-sm text-[11px] text-muted-foreground hover:bg-accent focus-visible:ring-1 focus-visible:ring-ring"
        onClick={() => {
          const next = !gatesRef.current.user;
          changeGate("user", next);
          setUserPaused(!next);
        }}>
        {userPaused ? "Resume" : "Pause"}
      </button>
    </div>
  );
}

/** No variant renders a static smile, without playback subscriptions or controls. */
export function AttentionMascotArtwork({ variant, paused = false, stageRef, cycleRef, cycleKey,
  currentId = variant, onAnimationEnd }: MascotArtworkProps = {}) {
  const id = useId().replace(/:/g, "");
  const silver = `mascot-silver-${id}`;
  const side = `mascot-side-${id}`;

  return (
    <div ref={stageRef} className="attention-mascot-stage" data-testid="attention-mascot-stage" data-mascot-id={currentId} data-variant={variant} data-paused={paused}>
      <svg className="attention-mascot" viewBox="0 0 104 88" role="img" aria-hidden="true">
        <defs>
          <linearGradient id={silver} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="var(--mascot-silver-light)" />
            <stop offset=".48" stopColor="var(--mascot-silver-mid)" />
            <stop offset="1" stopColor="var(--mascot-silver-dark)" />
          </linearGradient>
          <linearGradient id={side} x1="0" y1="0" x2="1" y2="0">
            <stop stopColor="var(--mascot-silver-bright)" />
            <stop offset="1" stopColor="var(--mascot-silver-mid)" />
          </linearGradient>
        </defs>
        <g key={cycleKey ?? variant ?? "static"} ref={cycleRef} onAnimationEnd={onAnimationEnd} className="mascot-dance-cycle" data-testid="qa-mascot-cycle">
        <g className="mascot-body">
        <g className="mascot-left-arm">
          <path className="mascot-left-upper-arm" d="M25 48 18 53" />
          <circle className="mascot-left-elbow" cx="18" cy="53" r="2.3" />
          <g className="mascot-left-elbow-pivot">
          <path className="mascot-left-forearm" d="m18 53-5 10" />
          <circle cx="13" cy="64" r="2.2" />
          </g>
        </g>
        <g className="mascot-right-arm">
          <path className="mascot-right-upper-arm" d="m79 48 7 5" />
          <circle className="mascot-right-elbow" cx="86" cy="53" r="2.3" />
          <g className="mascot-right-elbow-pivot">
          <path className="mascot-right-forearm" d="m86 53 5 10" />
          <circle cx="91" cy="64" r="2.2" />
          </g>
        </g>
        <g className="mascot-left-leg">
          <path className="mascot-left-thigh" d="m43 61-2 8" />
          <circle className="mascot-left-knee" cx="41" cy="69" r="2.2" />
          <g className="mascot-left-knee-pivot">
          <path className="mascot-left-lower-leg" d="m41 69-5 8" />
          <path className="mascot-foot" d="m36 77-5 1" />
          </g>
        </g>
        <g className="mascot-right-leg">
          <path className="mascot-right-thigh" d="m61 61 2 8" />
          <circle className="mascot-right-knee" cx="63" cy="69" r="2.2" />
          <g className="mascot-right-knee-pivot">
          <path className="mascot-right-lower-leg" d="m63 69 5 8" />
          <path className="mascot-foot" d="m68 77 5 1" />
          </g>
        </g>
        <g className="mascot-arch">
          <path className="mascot-arch-shell" fill={`url(#${silver})`} d="M22 48 31 39 34 28 43 31 52 13 61 31 70 28 73 39 82 48 68 63 64 61 72 47 61 37 52 34 43 37 32 47 40 61 36 63Z" />
          <path className="mascot-facet mascot-facet-left" fill={`url(#${side})`} d="m22 48 9-9 3-11 6 8-8 11 8 14-4 2zm9-9 3-11 9 3-8 13zm12-8 9-18v21l-9 3zm9 3 9-3 9-3 3 11-12-5z" />
          <path className="mascot-facet mascot-facet-right" fill={`url(#${side})`} d="m82 48-9-9-3-11-6 8 8 11-8 14 4 2zm-9-9-3-11-9 3 8 13zm-12-8-9-18v21l9 3z" />
          <path className="mascot-inner" d="M33 48 43 39 52 36 61 39 71 48 63 61" />
          <path className="mascot-edge" d="m22 48 9-9 3-11 9 3 9-18 9 18 9-3 3 11 9 9m-60 0 14 15 4-2m42-13-14 15-4-2" />
          <path className="mascot-eye" d="M44 47v1m16-1v1" />
          <path data-testid="qa-mascot-smile" className="mascot-smile" d="M46 53q6 6 12 0" />
        </g>
        </g>
        </g>
      </svg>
    </div>
  );
}
