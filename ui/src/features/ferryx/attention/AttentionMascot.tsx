import { useId } from "react";

import "./AttentionMascot.css";

/** Static, decorative silver-arch character. Motion is added in a later task. */
export function AttentionMascot() {
  const id = useId().replace(/:/g, "");
  const silver = `mascot-silver-${id}`;
  const side = `mascot-side-${id}`;

  return (
    <div className="attention-mascot-stage" data-testid="attention-mascot-stage" aria-hidden="true">
      <svg className="attention-mascot" viewBox="0 0 104 88" role="img">
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
        <g className="mascot-left-arm">
          <path className="mascot-left-upper-arm" d="M25 48 18 53" />
          <circle className="mascot-left-elbow" cx="18" cy="53" r="2.3" />
          <path className="mascot-left-forearm" d="m18 53-5 10" />
          <circle cx="13" cy="64" r="2.2" />
        </g>
        <g className="mascot-right-arm">
          <path className="mascot-right-upper-arm" d="m79 48 7 5" />
          <circle className="mascot-right-elbow" cx="86" cy="53" r="2.3" />
          <path className="mascot-right-forearm" d="m86 53 5 10" />
          <circle cx="91" cy="64" r="2.2" />
        </g>
        <g className="mascot-left-leg">
          <path className="mascot-left-thigh" d="m43 61-2 8" />
          <circle className="mascot-left-knee" cx="41" cy="69" r="2.2" />
          <path className="mascot-left-lower-leg" d="m41 69-5 8" />
          <path className="mascot-foot" d="m36 77-5 1" />
        </g>
        <g className="mascot-right-leg">
          <path className="mascot-right-thigh" d="m61 61 2 8" />
          <circle className="mascot-right-knee" cx="63" cy="69" r="2.2" />
          <path className="mascot-right-lower-leg" d="m63 69 5 8" />
          <path className="mascot-foot" d="m68 77 5 1" />
        </g>
        <g className="mascot-arch">
          <path className="mascot-arch-shell" fill={`url(#${silver})`} d="M22 48 31 39 34 28 43 31 52 13 61 31 70 28 73 39 82 48 68 63 64 61 72 47 61 37 52 34 43 37 32 47 40 61 36 63Z" />
          <path className="mascot-facet mascot-facet-left" fill={`url(#${side})`} d="m22 48 9-9 3-11 6 8-8 11 8 14-4 2zm9-9 3-11 9 3-8 13zm12-8 9-18v21l-9 3zm9 3 9-3 9-3 3 11-12-5z" />
          <path className="mascot-facet mascot-facet-right" fill={`url(#${side})`} d="m82 48-9-9-3-11-6 8 8 11-8 14 4 2zm-9-9-3-11-9 3 8 13zm-12-8-9-18v21l9 3z" />
          <path className="mascot-inner" d="M33 48 43 39 52 36 61 39 71 48 63 61" />
          <path className="mascot-edge" d="m22 48 9-9 3-11 9 3 9-18 9 18 9-3 3 11 9 9m-60 0 14 15 4-2m42-13-14 15-4-2" />
          <path className="mascot-eye" d="M44 47v1m16-1v1" />
          <path className="mascot-smile" d="M46 53q6 6 12 0" />
        </g>
      </svg>
    </div>
  );
}
