import type { Entry } from "../settings/registry";
import { RANGES } from "../../ui/ranges";

// Settings' Performance area; its groups are in areas.ts.
const perf = { view: "settings", area: "performance" } as const;
const flags: Pick<Entry, "place" | "parent" | "when"> = { place: perf, parent: "lighterRendering", when: (c) => c.s.lighterRendering };
const mb = { unit: { text: "MB" }, short: { text: "MB" } } as const;

/** Performance's settings for the settings registry (search, jumps, rows), in screen order. */
export const PERFORMANCE_ENTRIES: readonly Entry[] = [
  // Search-only: the cards above the rows (panels of the area, not rows).
  { id: "presets", place: perf, label: "performance.presets", control: { kind: "action" } },
  { id: "maxAlts", place: perf, label: "performance.preset.maxAlts", hint: "performance.tagMaxAlts", control: { kind: "action" } },
  { id: "balanced", place: perf, label: "performance.preset.balanced", hint: "performance.tagBalanced", control: { kind: "action" } },
  { id: "defaults", place: perf, label: "performance.preset.defaults", hint: "performance.tagDefaults", control: { kind: "action" } },
  { id: "bench", place: perf, label: "performance.bench.title", hint: "performance.bench.intro", control: { kind: "action" }, aliases: ["benchmark", "measure"] },

  { id: "fpsCap", place: perf, label: "settings.fpsCap", hint: "performance.hintFps", more: "settings.fpsCapHint", field: "fpsCap", control: { kind: "offOrValue", ...RANGES.fpsCap, unit: { text: "FPS" }, short: { text: "FPS" } }, aliases: ["fps", "frame"] },
  { id: "lowGraphics", place: perf, label: "settings.lowGraphics", hint: "performance.hintLow", more: "settings.lowGraphicsHint", field: "lowGraphics", control: { kind: "switch" }, aliases: ["quality", "gpu"] },
  { id: "lighterRendering", place: perf, label: "settings.lighterRendering", hint: "performance.hintLight", more: "performance.graphicsHint", field: "lighterRendering", control: { kind: "switch" }, aliases: ["fastflags", "flags"] },
  {
    id: "msaa", ...flags, label: "performance.msaa", hint: "performance.hintMsaa", field: "renderFlags.msaa", aliases: ["antialiasing"],
    control: { kind: "select", options: [{ value: "default", label: "performance.robloxDefault" }, { value: "x1", label: { text: "x1" } }, { value: "x2", label: { text: "x2" } }, { value: "x4", label: { text: "x4" } }] },
  },
  {
    id: "texture", ...flags, label: "performance.texture", hint: "performance.hintTexture", field: "renderFlags.texture",
    control: {
      kind: "select",
      options: [
        { value: "default", label: "performance.robloxDefault" },
        { value: "lowest", label: "performance.texLowest" },
        { value: "low", label: "performance.texLow" },
        { value: "medium", label: "performance.texMedium" },
        { value: "high", label: "performance.texHigh" },
      ],
    },
  },
  {
    id: "renderMode", ...flags, label: "performance.renderMode", hint: "performance.hintRenderMode", field: "renderFlags.mode", aliases: ["d3d11", "vulkan", "opengl", "directx"],
    control: {
      kind: "select",
      options: [
        { value: "default", label: "performance.robloxDefault" },
        { value: "d3d11", label: { text: "Direct3D 11" } },
        { value: "vulkan", label: { text: "Vulkan" } },
        { value: "opengl", label: { text: "OpenGL" } },
      ],
    },
  },
  { id: "graySky", ...flags, label: "performance.graySky", hint: "performance.hintGraySky", field: "renderFlags.graySky", control: { kind: "switch" }, aliases: ["skybox"] },
  { id: "noGrass", ...flags, label: "performance.noGrass", hint: "performance.hintNoGrass", field: "renderFlags.noGrass", control: { kind: "switch" } },
  { id: "lowestFrm", ...flags, label: "performance.lowestFrm", hint: "performance.hintLowestFrm", field: "renderFlags.lowestFrm", control: { kind: "switch" }, aliases: ["frm"] },
  { id: "pauseVoxelizer", ...flags, label: "performance.pauseVoxelizer", hint: "performance.hintPauseVoxelizer", field: "renderFlags.pauseVoxelizer", control: { kind: "switch" }, aliases: ["voxel"] },
  { id: "customFlags", place: perf, label: "flags.title", hint: (c) => (c.s.customFlagsScope === "account" ? "flags.hintAccount" : "flags.hint"), more: "flags.more", field: ["customFlags", "customFlagsScope"], control: { kind: "custom" }, aliases: ["fflags", "fastflags", "flags", "ClientAppSettings", "bloxstrap"] },
  { id: "minimize", place: perf, label: "settings.minimize", hint: "performance.hintMinimize", more: "settings.minimizeHint", field: "minimizeOnLaunch", control: { kind: "switch" }, aliases: ["hide"] },
  { id: "tinyOnLaunch", place: perf, label: "performance.tinyOnLaunch", hint: "performance.hintTinyOnLaunch", field: "tinyOnLaunch", control: { kind: "switch" }, aliases: ["micro", "small", "tiny window size", "width", "height"] },
  { id: "mute", place: perf, label: "performance.muteOnLaunch", hint: "performance.hintMuteOnLaunch", field: "muteOnLaunch", control: { kind: "switch" }, aliases: ["sound", "audio"] },

  { id: "adaptive", place: perf, label: "settings.adaptive", hint: "performance.adaptiveShort", more: "settings.adaptiveHint", field: "adaptive.enabled", control: { kind: "switch" }, aliases: ["background"] },
  {
    id: "adaptiveBg", place: perf, parent: "adaptive", when: (c) => c.s.adaptive.enabled, label: "settings.inBackground", more: "settings.adaptiveBgHint",
    field: ["adaptive.background.cpuCap", "adaptive.background.eco", "adaptive.background.trimEveryMin"], control: { kind: "custom" }, aliases: ["eco", "efficiency", "cpu"],
  },
  { id: "fairCap", place: perf, label: "settings.fairCap", hint: "performance.fairCapShort", more: "settings.fairCapHint", field: "fairCap", control: { kind: "switch" }, aliases: ["cpu"] },
  { id: "spread", place: perf, label: "settings.spread", more: "settings.spreadHint", field: "autoSpread", control: { kind: "switch" }, aliases: ["affinity", "cores"] },
  {
    id: "trim", place: perf, label: "settings.trim", hint: "performance.trimShort", more: ["settings.trimHint", "settings.commitStays"], field: "trimEveryMin",
    control: { kind: "offOrValue", ...RANGES.trimMin, unit: "settings.unit.minutes", short: "settings.unit.min" }, aliases: ["ram", "working set"],
  },
  {
    id: "memThrottle", place: perf, label: "settings.memThrottle", hint: "performance.memThrottleShort", more: ["settings.memThrottleHint", "settings.noMemoryFreed"], field: "memThrottle.mb",
    control: { kind: "offOrValue", ...RANGES.memThrottleMb, ...mb }, aliases: ["ram", "commit"],
  },
  {
    id: "memThrottleCpu", place: perf, parent: "memThrottle", when: (c) => c.s.memThrottle.mb > 0, label: "settings.memThrottleCpu", field: "memThrottle.cpuCap",
    control: { kind: "segmented", mono: true, options: [25, 10, 5, 2].map((n) => ({ value: n, label: { key: "settings.percent", params: { n } } })) },
  },
  {
    id: "memRestart", place: perf, label: "settings.memRestart", hint: "performance.memRestartShort", more: "settings.memRestartHint", field: "memRestartMb",
    control: { kind: "offOrValue", ...RANGES.memRestartMb, ...mb }, aliases: ["ram", "commit", "relaunch"],
  },

  { id: "previewFps", place: perf, label: "dashboard.previewFps", hint: "performance.previewFpsHint", more: "performance.previewFpsMore", field: "local:previewFps", control: { kind: "custom" }, aliases: ["fps", "preview"] },

  { id: "bandwidth", place: perf, label: "settings.bandwidth", more: "settings.bandwidthHint", field: "bandwidthKbps", control: { kind: "custom" }, aliases: ["qos", "upload", "kbps", "network"] },

  { id: "share", place: perf, label: "performance.shareTitle", hint: "performance.shareHint", control: { kind: "action" }, aliases: ["export", "import", "code", "profile"] },
];
