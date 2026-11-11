/** One spot of the window layout preview (`layout_preview`): physical px from the monitor's work
 * area; `order` is the place in the launch order of a picked account, else null. */
export type PreviewCell = { x: number; y: number; w: number; h: number; order: number | null; name: string; running: boolean };
export type PreviewData = { cells: PreviewCell[]; running: string };

/** The layout preview window's page: every spot drawn over the screen, the window itself see-through.
 * It has no IPC: all it shows comes in `data`. */
export function LayoutPreview({ data }: { data: PreviewData }) {
  // main.tsx made the page see-through. Fixed colours: this window loads no settings, so it doesn't
  // know the theme or accent, and it is drawn over whatever is on the screen. White with a black
  // hairline outside reads on a light desktop and a dark one.
  const px = (v: number) => v / (window.devicePixelRatio || 1);
  return (
    <div className="fixed inset-0 pointer-events-none select-none">
      {data.cells.map((c, i) => (
        <div
          key={i}
          className="absolute flex flex-col items-center justify-center gap-1 overflow-hidden rounded-md text-center"
          style={{
            left: px(c.x) + 3,
            top: px(c.y) + 3,
            width: Math.max(px(c.w) - 6, 8),
            height: Math.max(px(c.h) - 6, 8),
            border: `3px ${c.running ? "dashed" : "solid"} #fff`,
            boxShadow: "0 0 0 1px #000",
            background: "rgb(0 0 0 / 0.2)",
            opacity: c.running ? 0.55 : 1,
            color: "#fff",
            textShadow: "0 1px 3px #000, 0 0 8px #000",
          }}
        >
          {c.order !== null && <span className="text-5xl font-bold leading-none">{c.order}</span>}
          <span className="max-w-full truncate px-2 text-lg font-semibold">{c.name}</span>
          {c.running && <span className="text-sm">{data.running}</span>}
        </div>
      ))}
    </div>
  );
}
