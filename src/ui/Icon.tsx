// Square-cut 16px line icons, drawn to match the design canvas.
const paths = {
  search: <><rect x="2" y="2" width="9" height="9" /><path d="M10.5 10.5L14 14" /></>,
  sun: <><rect x="5" y="5" width="6" height="6" /><path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.5 1.5M11.5 11.5L13 13M13 3l-1.5 1.5M4.5 11.5L3 13" /></>,
  moon: <path d="M13.5 9.5A6 6 0 0 1 6.5 2.5a6 6 0 1 0 7 7z" />,
  lock: <><rect x="3" y="7" width="10" height="7" /><path d="M5 7V3h6v4" /></>,
  accounts: <><rect x="5" y="2" width="6" height="6" /><path d="M2 14v-2h12v2" /></>,
  settings: <><rect x="5" y="5" width="6" height="6" /><path d="M8 1v3M8 12v3M1 8h3M12 8h3" /></>,
  plus: <path d="M8 2v12M2 8h12" />,
  chat: <path d="M2 3h12v8H7l-3 3v-3H2z" strokeLinejoin="round" />,
  heart: <path d="M8 14L2.4 8.4a3.2 3.2 0 0 1 4.5-4.5L8 5l1.1-1.1a3.2 3.2 0 0 1 4.5 4.5z" strokeLinejoin="round" />,
  refresh: <><path d="M13 3v4H9" /><path d="M13 7A5.5 5.5 0 1 0 13.5 10" /></>,
  star: <path d="M8 1.5l1.9 4.2 4.6.4-3.5 3 1.1 4.5L8 11.2l-4.1 2.4L5 9.1l-3.5-3 4.6-.4z" />,
  copy: <><rect x="5" y="5" width="9" height="9" /><path d="M11 5V2H2v9h3" /></>,
  external: <path d="M6 3H3v10h10v-3M9 3h4v4M13 3L7 9" />,
  eye: <><path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z" /><rect x="6.5" y="6.5" width="3" height="3" /></>,
  eyeOff: <><path d="M1 8s2.5-5 7-5 7 5 7 5-2.5 5-7 5-7-5-7-5z" /><path d="M2 14L14 2" /></>,
  x: <path d="M4 4l8 8M12 4l-8 8" />,
  sliders: <><path d="M2 4h12M2 12h12" /><rect x="9" y="2.5" width="3" height="3" /><rect x="4" y="10.5" width="3" height="3" /></>,
  check: <path d="M3 8.5l3 3 7-7" />,
  trash: <><path d="M2 4h12M6 4V2h4v2" /><path d="M3.5 4l1 10h7l1-10" /></>,
  pencil: <path d="M10.5 2.5l3 3L6 13H3v-3z" />,
  download: <path d="M8 2v8M4.5 6.5L8 10l3.5-3.5M2 13.5h12" />,
  upload: <path d="M8 10V2M4.5 5.5L8 2l3.5 3.5M2 13.5h12" />,
  signin: <path d="M9 2h5v12H9M2 8h8M7 5l3 3-3 3" />,
  key: <><rect x="1.5" y="5.5" width="5" height="5" /><path d="M6.5 8h8M11.5 8v3M14 8v2" /></>,
  chevron: <path d="M4 6l4 4 4-4" />,
  monitor: <path d="M1 9h3l2-5 3 9 2-5h4" />,
  bell: <><path d="M3 12h10l-1.5-2V6.5a3.5 3.5 0 0 0-7 0V10z" /><path d="M6.5 14h3" /></>,
  play: <path d="M4 2.5l9 5.5-9 5.5z" strokeLinejoin="round" />,
  home: <path d="M2 7.5L8 2.5l6 5M3.5 6.5v7h9v-7" strokeLinejoin="round" />,
  grid: <><rect x="2" y="2" width="5" height="5" /><rect x="9" y="2" width="5" height="5" /><rect x="2" y="9" width="5" height="5" /><rect x="9" y="9" width="5" height="5" /></>,
  gauge: <><path d="M2.5 12a5.5 5.5 0 1 1 11 0" /><path d="M8 12l3-4" /></>,
  servers: <><rect x="2" y="2" width="12" height="5" /><rect x="2" y="9" width="12" height="5" /><path d="M5 4.5h1M5 11.5h1" /></>,
  dots: (
    <g fill="currentColor" stroke="none">
      <circle cx="3" cy="8" r="1.5" /><circle cx="8" cy="8" r="1.5" /><circle cx="13" cy="8" r="1.5" />
    </g>
  ),
  grip: (
    <g fill="currentColor" stroke="none">
      <rect x="5" y="3" width="2" height="2" /><rect x="9" y="3" width="2" height="2" />
      <rect x="5" y="7" width="2" height="2" /><rect x="9" y="7" width="2" height="2" />
      <rect x="5" y="11" width="2" height="2" /><rect x="9" y="11" width="2" height="2" />
    </g>
  ),
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 16, fill = false, className }: { name: IconName; size?: number; fill?: boolean; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill={fill ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={name === "plus" ? 1.75 : 1.5}
      aria-hidden="true"
      className={className}
    >
      {paths[name]}
    </svg>
  );
}

/** The RoALT mark: a lime square under an outlined one. */
export function Logo() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <rect x="1" y="1" width="9" height="9" rx="1" fill="var(--accent)" />
      {/* The app icon's tilted tile with a hole (src-tauri/icons/app-icon.svg). */}
      <mask id="logo-hole">
        <rect width="18" height="18" fill="#fff" />
        {/* In the tile's own (rotated) space. */}
        <rect x="10.3" y="10.3" width="2.4" height="2.4" rx="0.4" fill="#000" />
      </mask>
      <rect transform="rotate(15 11.5 11.5)" x="7.5" y="7.5" width="8" height="8" rx="1" fill="currentColor" mask="url(#logo-hole)" />
    </svg>
  );
}
