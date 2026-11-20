// What the backend accepts for the "Off, or a value" settings (src-tauri commands.rs validators).
// `on`: the value a switched-on field starts at.
export const RANGES = {
  fpsCap: { min: 5, max: 360, on: 60 },
  trimMin: { min: 1, max: 1440, on: 15 },
  memThrottleMb: { min: 512, max: 65_536, on: 4096 },
  memRestartMb: { min: 512, max: 65_536, on: 6144 },
  statusEveryMin: { min: 5, max: 1440, on: 60 },
  timerHours: { min: 1, max: 168, on: 4 },
  bandwidthKbps: { min: 64, max: 10_000_000, on: 1000 },
} as const;

export type Range = { min: number; max: number; on: number; step?: number };
