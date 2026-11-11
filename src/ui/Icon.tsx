import { useId } from "react";
import { isRtl } from "../i18n";

// 16px outline icons: 1.5px strokes with round caps and joins, one consistent set. Dots are drawn as
// zero-length strokes (round caps make them circles).
const paths = {
  search: <><circle cx="7" cy="7" r="4.5" /><path d="M10.4 10.4 14 14" /></>,
  sun: <><circle cx="8" cy="8" r="2.9" /><path d="M8 1.5v1.4M8 13.1v1.4M1.5 8h1.4M13.1 8h1.4M3.4 3.4l1 1M11.6 11.6l1 1M12.6 3.4l-1 1M4.4 11.6l-1 1" /></>,
  moon: <path d="M13.5 9.7A5.6 5.6 0 0 1 6.3 2.5a5.75 5.75 0 1 0 7.2 7.2z" />,
  lock: <><rect x="3" y="7" width="10" height="7" rx="2" /><path d="M5.25 7V5.1a2.75 2.75 0 0 1 5.5 0V7" /></>,
  accounts: <><circle cx="8" cy="5.4" r="2.7" /><path d="M2.9 14c.3-2.7 2.4-4.4 5.1-4.4s4.8 1.7 5.1 4.4" /></>,
  settings: (
    <>
      <circle cx="8" cy="8" r="3.9" />
      <circle cx="8" cy="8" r="5.6" strokeWidth="1.9" strokeLinecap="butt" strokeDasharray="2.15 2.25" strokeDashoffset="1.1" />
      <circle cx="8" cy="8" r="1.5" />
    </>
  ),
  plus: <path d="M8 3v10M3 8h10" />,
  chat: <path d="M3.6 3h8.8A1.6 1.6 0 0 1 14 4.6v5.3a1.6 1.6 0 0 1-1.6 1.6H7.3l-2.8 2.3v-2.3h-.9A1.6 1.6 0 0 1 2 9.9V4.6A1.6 1.6 0 0 1 3.6 3z" />,
  heart: <path d="M8 13.4 2.9 8.5a3.1 3.1 0 0 1 4.4-4.4l.7.7.7-.7a3.1 3.1 0 0 1 4.4 4.4z" />,
  refresh: <><path d="M2.7 8a5.3 5.3 0 0 1 9.4-3.3M13.3 2.6v3h-3" /><path d="M13.3 8a5.3 5.3 0 0 1-9.4 3.3M2.7 13.4v-3h3" /></>,
  star: <path d="M8 1.9l1.85 3.85 4.25.55-3.1 2.95.8 4.2L8 11.4l-3.8 2.05.8-4.2-3.1-2.95 4.25-.55z" />,
  copy: <><rect x="5.5" y="5.5" width="8" height="8" rx="1.8" /><path d="M10.5 5.5V4.3a1.8 1.8 0 0 0-1.8-1.8H4.3a1.8 1.8 0 0 0-1.8 1.8v4.4a1.8 1.8 0 0 0 1.8 1.8h1.2" /></>,
  external: <path d="M9.5 2.5h4v4M13.5 2.5 7.6 8.4M12 9.6v2.6a1.3 1.3 0 0 1-1.3 1.3H3.8a1.3 1.3 0 0 1-1.3-1.3V5.3A1.3 1.3 0 0 1 3.8 4h2.6" />,
  eye: <><path d="M1.5 8s2.4-4.6 6.5-4.6S14.5 8 14.5 8s-2.4 4.6-6.5 4.6S1.5 8 1.5 8z" /><circle cx="8" cy="8" r="2" /></>,
  eyeOff: <><path d="M6.4 3.6A6.3 6.3 0 0 1 8 3.4c4.1 0 6.5 4.6 6.5 4.6a11 11 0 0 1-1.6 2.2M10.1 11.9a6 6 0 0 1-2.1.7C3.9 12.6 1.5 8 1.5 8a11.2 11.2 0 0 1 2.6-3.1" /><path d="M2.5 2.5l11 11" /></>,
  x: <path d="M4 4l8 8M12 4l-8 8" />,
  sliders: <><path d="M2.5 5h6M11.5 5h2M2.5 11h2M7.5 11h6" /><circle cx="10" cy="5" r="1.5" /><circle cx="6" cy="11" r="1.5" /></>,
  check: <path d="M3 8.6l3 3L13 4.6" />,
  trash: <path d="M2.5 4.5h11M6.4 4.5v-1A1 1 0 0 1 7.4 2.5h1.2a1 1 0 0 1 1 1v1M4 4.5l.7 8.1a1.5 1.5 0 0 0 1.5 1.4h3.6a1.5 1.5 0 0 0 1.5-1.4l.7-8.1" />,
  pencil: <path d="M10.6 2.7a1.8 1.8 0 0 1 2.6 2.6l-7.6 7.6-3.1.7.7-3.1z" />,
  download: <path d="M8 2.5v8M4.8 7.3 8 10.5l3.2-3.2M2.5 13.5h11" />,
  upload: <path d="M8 10.5v-8M4.8 5.7 8 2.5l3.2 3.2M2.5 13.5h11" />,
  signin: <path d="M9.5 2.5h2.7a1.3 1.3 0 0 1 1.3 1.3v8.4a1.3 1.3 0 0 1-1.3 1.3H9.5M2.5 8h7.2M7 5l3 3-3 3" />,
  key: <><circle cx="5" cy="8" r="2.75" /><path d="M7.75 8h5.75v2.4M11 8v1.6" /></>,
  chevron: <path d="M4 6l4 4 4-4" />,
  monitor: <path d="M1.5 8.6h2.6L5.8 4l3 8 1.7-3.4h4" />,
  bell: <><path d="M4 11.5V7a4 4 0 0 1 8 0v4.5l1 1H3z" /><path d="M6.5 14.2h3" /></>,
  play: <path d="M5 3.2v9.6a.6.6 0 0 0 .9.5l7.5-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5z" />,
  news: <><path d="M3 3.5h8.5v8.9A1.6 1.6 0 0 0 13.1 14H4.6A1.6 1.6 0 0 1 3 12.4z" /><path d="M11.5 6.5H13v5.9a1.6 1.6 0 0 1-1.5 1.6M5.4 6.2h3.7M5.4 8.7h3.7M5.4 11.2h2.2" /></>,
  home: <path d="M2.5 7.2 8 2.6l5.5 4.6M4 6v6.4A1.1 1.1 0 0 0 5.1 13.5h5.8a1.1 1.1 0 0 0 1.1-1.1V6M6.6 13.5V10h2.8v3.5" />,
  grid: <><rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1.3" /><rect x="9" y="2.5" width="4.5" height="4.5" rx="1.3" /><rect x="2.5" y="9" width="4.5" height="4.5" rx="1.3" /><rect x="9" y="9" width="4.5" height="4.5" rx="1.3" /></>,
  gauge: <><path d="M2.7 10.65a5.3 5.3 0 1 1 10.6 0" /><path d="M8 10.65l2.6-3.4" /></>,
  servers: <><rect x="2.5" y="2.5" width="11" height="4.5" rx="1.3" /><rect x="2.5" y="9" width="11" height="4.5" rx="1.3" /><path d="M5 4.75h.01M5 11.25h.01" /></>,
  dots: (
    <g fill="currentColor" stroke="none">
      <circle cx="3.2" cy="8" r="1.35" /><circle cx="8" cy="8" r="1.35" /><circle cx="12.8" cy="8" r="1.35" />
    </g>
  ),
  grip: (
    <g fill="currentColor" stroke="none">
      <circle cx="6" cy="4" r="1.1" /><circle cx="10" cy="4" r="1.1" />
      <circle cx="6" cy="8" r="1.1" /><circle cx="10" cy="8" r="1.1" />
      <circle cx="6" cy="12" r="1.1" /><circle cx="10" cy="12" r="1.1" />
    </g>
  ),
  // New in the island redesign.
  tools: <path d="M10.3 2.6a3.2 3.2 0 0 0-3.9 4.1L2.9 10.2a1.45 1.45 0 0 0 2.05 2.05l3.5-3.5a3.2 3.2 0 0 0 4.1-3.9l-1.95 1.95-1.85-.4-.4-1.85z" />,
  terminal: <><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="2" /><path d="M4.6 6.4 6.6 8.2l-2 1.8M8.6 10h3" /></>,
  shield: <path d="M8 1.8 13 3.6v4.1c0 3.1-2.1 5.4-5 6.5-2.9-1.1-5-3.4-5-6.5V3.6z" />,
  bolt: <path d="M9 1.8 3.8 9h4.1l-.8 5.2L12.2 7H8.1z" />,
  database: <><ellipse cx="8" cy="3.9" rx="5" ry="1.9" /><path d="M3 3.9v8.2c0 1 2.2 1.9 5 1.9s5-.9 5-1.9V3.9M3 8c0 1 2.2 1.9 5 1.9S13 9 13 8" /></>,
  info: <><circle cx="8" cy="8" r="6" /><path d="M8 7.3v3.7M8 5h.01" /></>,
  rocket: <><path d="M13.6 2.4c-3.3-.1-6 2-7.5 5.4l2.1 2.1c3.4-1.5 5.5-4.2 5.4-7.5z" /><path d="M6 7.8 3.4 8.3l1.8-2.4 2.2-.1M8.2 10l-.5 2.6 2.4-1.8.1-2.2M4.5 11.5l-1.5 1.5" /><circle cx="10.4" cy="5.6" r=".9" /></>,
  keyboard: <><rect x="1.5" y="4" width="13" height="8" rx="1.8" /><path d="M4.5 6.8h.01M7 6.8h.01M9.5 6.8h.01M12 6.8h.01M5.2 9.4h5.6" /></>,
  palette: <><path d="M8 2a6 6 0 1 0 0 12c.9 0 1.4-.6 1.4-1.3 0-.4-.2-.7-.4-1s-.4-.6-.4-1c0-.8.6-1.3 1.4-1.3h1.6A2.4 2.4 0 0 0 14 7a6 6 0 0 0-6-5z" /><path d="M5 7.4h.01M6.6 4.8h.01M9.6 4.8h.01" /></>,
  globe: <><circle cx="8" cy="8" r="6" /><path d="M2 8h12M8 2c1.6 1.6 2.4 3.6 2.4 6S9.6 12.4 8 14C6.4 12.4 5.6 10.4 5.6 8S6.4 3.6 8 2z" /></>,
  alert: <><path d="M7.1 2.9a1 1 0 0 1 1.8 0l5 9.2a1 1 0 0 1-.9 1.4H3a1 1 0 0 1-.9-1.4z" /><path d="M8 6.6v3M8 11.4h.01" /></>,
  stop: <rect x="3.5" y="3.5" width="9" height="9" rx="2" />,
  minimize: <path d="M6.4 2.6v3.8H2.6M9.6 2.6v3.8h3.8M6.4 13.4V9.6H2.6M9.6 13.4V9.6h3.8" />,
  layoutSide: <><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="2" /><path d="M6 2.8v10.4" /></>,
  layoutTop: <><rect x="1.8" y="2.8" width="12.4" height="10.4" rx="2" /><path d="M1.8 6.4h12.4" /></>,
  back: <path d="M10 3.5 5.5 8l4.5 4.5" />,
  clock: <><circle cx="8" cy="8" r="6" /><path d="M8 4.8V8l2.2 1.4" /></>,
  // Discord's mark (a brand glyph, filled; drawn on 24 units and scaled to 16).
  discord: (
    <path
      fill="currentColor"
      stroke="none"
      transform="scale(0.6667)"
      d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189z"
    />
  ),
} as const;

export type IconName = keyof typeof paths;

/** Icons that point along the reading direction mirror right to left. */
const MIRRORED = new Set<IconName>(["back", "signin"]);

export function Icon({ name, size = 16, fill = false, className, flipRtl }: { name: IconName; size?: number; fill?: boolean; className?: string; flipRtl?: boolean }) {
  const flip = (flipRtl ?? MIRRORED.has(name)) && isRtl();
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill={fill ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
      style={flip ? { transform: "scaleX(-1)" } : undefined}
    >
      {paths[name]}
    </svg>
  );
}

/** Window caption glyphs (10px, as Windows draws them). */
export function CaptionGlyph({ name }: { name: "minimize" | "maximize" | "restore" | "close" }) {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" aria-hidden="true">
      {name === "minimize" && <path d="M0 5h10" />}
      {name === "maximize" && <rect x="0.5" y="0.5" width="9" height="9" rx="1" />}
      {name === "restore" && (
        <>
          <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
          <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
        </>
      )}
      {name === "close" && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />}
    </svg>
  );
}

/** The RoALT mark: an accent square under the tilted tile (src-tauri/icons/app-icon.svg). */
export function Logo({ size = 18 }: { size?: number }) {
  const hole = useId();
  return (
    <svg width={size} height={size} viewBox="0 0 18 18" aria-hidden="true">
      <rect x="1" y="1" width="9" height="9" rx="1.6" fill="var(--accent)" />
      <mask id={hole}>
        <rect width="18" height="18" fill="#fff" />
        {/* In the tile's own (rotated) space. */}
        <rect x="10.3" y="10.3" width="2.4" height="2.4" rx="0.5" fill="#000" />
      </mask>
      <rect transform="rotate(15 11.5 11.5)" x="7.5" y="7.5" width="8" height="8" rx="1.6" fill="var(--fg-3)" mask={`url(#${hole})`} />
    </svg>
  );
}
