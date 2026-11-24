#!/usr/bin/env python3
"""Update docs/images/linux-port-status.svg from docs/PROGRESS.md.

The SVG is a fixed 45-tile board.  This script only changes the three things
that carry data: the header totals, each tile's `<title>LABEL: N%</title>`,
and the green/gray fill of its 16x7 cells.  Layout and text stay untouched.

Usage:
    python3 scripts/render-port-status.py          # write the SVG
    python3 scripts/render-port-status.py --check  # exit 1 if it would change
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROGRESS = ROOT / "docs" / "PROGRESS.md"
SVG = ROOT / "docs" / "images" / "linux-port-status.svg"

# PROGRESS.md category -> SVG tile label.  Keep in sync when a tile is added.
LABELS = {
    "Accounts": "ACCOUNTS",
    "First-run setup": "FIRST-RUN SETUP",
    "Limiteds": "LIMITEDS",
    "Launching": "LAUNCH",
    "Launch queue": "LAUNCH QUEUE",
    "Batches and same server": "BATCHES",
    "Take control": "TAKE CONTROL",
    "Multi-client": "MULTI",
    "Mocktail backend": "MOCKTAIL",
    "Headless": "HEADLESS",
    "Previews": "PREVIEWS",
    "Rejoin": "REJOIN",
    "Servers": "SERVERS",
    "Browse and details": "BROWSE",
    "FPS cap": "FPS",
    "Discord": "DISCORD",
    "Alerts": "ALERTS",
    "Limits": "LIMITS",
    "Metrics": "METRICS",
    "Window controls": "WINDOW",
    "Tray": "TRAY",
    "Roblox updates": "UPDATES",
    "Default launcher": "DEFAULT LAUNCHER",
    "Anti-AFK": "ANTI-AFK",
    "Free resizing": "RESIZE",
    "Window layouts": "LAYOUT",
    "Layout preview": "LAYOUT PREVIEW",
    "Re-authentication": "REAUTH",
    "Self-update": "SELF-UPDATE",
    "Interface": "INTERFACE",
    "Custom themes": "CUSTOM THEMES",
    "Online counts": "ONLINE COUNTS",
    "Island redesign": "ISLAND",
    "Languages": "LANGUAGES",
    "Max alts": "MAX ALTS",
    "Usage stats": "USAGE STATS",
    "Logs": "LOGS",
    "Modpacks": "MODPACKS",
    "News": "NEWS",
    "Cookie export": "COOKIE EXPORT",
    "UI sounds": "SOUNDS",
    "Settings search": "SETTINGS",
    "Vault flow and recovery": "VAULT",
    "Bandwidth / QoS": "BANDWIDTH",
    "Audio mute": "AUDIO MUTE",
}

GREEN = "#2ea043"
GRAY = "#30363d"
CELL = re.compile(
    r'(<rect x="[\d.]+" y="[\d.]+" width="8\.0" height="8\.0" fill=")(#[0-9a-f]{6})('
    r'" stroke="#0d1117" stroke-width="0\.5"><title>)([A-Z][A-Z -]+): (\d+)(%</title></rect>)'
)
HEADER = re.compile(r'(<text x="8" y="22" font-family="sans-serif" font-size="16" fill="#ffffff">)([^<]*)(</text>)')
NOTE = re.compile(r'(<text x="8" y="786" font-family="sans-serif" font-size="10" fill="#8b949e">)([^<]*)(</text>)')
FOOTER = (
    '<text x="8" y="798" font-family="sans-serif" font-size="10" fill="#8b949e">'
    'Green: working or complete. Gray: partial, untested, or unavailable.</text>'
)


def read_data() -> tuple[dict[str, int], str, str]:
    text = PROGRESS.read_text(encoding="utf-8")
    summary = text.split("## Summary", 1)[1].split("\n## ", 1)[0]
    values: dict[str, int] = {}
    for line in summary.splitlines():
        if not line.startswith("| "):
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if len(cells) < 3 or cells[0] not in LABELS:
            continue
        match = re.fullmatch(r"(\d+)%", cells[2])
        if not match:
            raise SystemExit(f"bad coverage for {cells[0]!r}: {cells[2]!r}")
        values[LABELS[cells[0]]] = int(match.group(1))

    totals = re.search(r"Current totals: \*\*(\d+) ported, (\d+) partial, untested or missing\*\*", text)
    if not totals:
        raise SystemExit("no 'Current totals' line in docs/PROGRESS.md")
    header = f"Linux port status: {totals.group(1)} ported, {totals.group(2)} partial, untested or missing"
    note_match = re.search(r"^SVG note: (.+)$", text, re.MULTILINE)
    if not note_match:
        raise SystemExit("no 'SVG note:' line in docs/PROGRESS.md")
    return values, header, note_match.group(1)


def render(svg: str, values: dict[str, int], header: str, note: str) -> str:
    seen: dict[str, int] = {}

    def cell(m: re.Match[str]) -> str:
        label, pct = m.group(4), int(m.group(5))
        if label not in values:
            raise SystemExit(f"SVG tile {label!r} has no row in PROGRESS.md")
        n = seen.get(label, 0)
        seen[label] = n + 1
        filled = round(values[label] * 112 / 100)
        return m.group(1) + (GREEN if n < filled else GRAY) + m.group(3) + label + ": " + str(values[label]) + m.group(6)

    out = CELL.sub(cell, svg)
    counts: dict[str, int] = {}
    for m in CELL.finditer(svg):
        counts[m.group(4)] = counts.get(m.group(4), 0) + 1
    missing = set(values) - set(counts)
    if missing:
        raise SystemExit(f"PROGRESS.md has no SVG tile for: {sorted(missing)}")
    for label, n in seen.items():
        if n != counts[label]:
            raise SystemExit(f"cell count for {label} changed: {n} != {counts[label]}")

    out, n = HEADER.subn(lambda m: m.group(1) + header + m.group(3), out)
    if n != 1:
        raise SystemExit(f"expected one status header, found {n}")

    out, n = NOTE.subn(lambda m: m.group(1) + note + m.group(3), out)
    if n == 0:
        if FOOTER not in out:
            raise SystemExit("SVG footer missing; cannot add the integration note")
        note_line = f'<text x="8" y="786" font-family="sans-serif" font-size="10" fill="#8b949e">{note}</text>'
        out = out.replace(FOOTER, note_line + "\n" + FOOTER, 1)
    elif n != 1:
        raise SystemExit(f"expected one integration note, found {n}")
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="do not write; fail if the SVG is stale")
    args = parser.parse_args()

    values, header, note = read_data()
    old = SVG.read_text(encoding="utf-8")
    new = render(old, values, header, note)
    if args.check:
        if new == old:
            print("SVG is up to date")
            return
        print("SVG is stale; run: python3 scripts/render-port-status.py", file=sys.stderr)
        raise SystemExit(1)
    if new != old:
        SVG.write_text(new, encoding="utf-8")
        print("updated docs/images/linux-port-status.svg")
    else:
        print("SVG already up to date")


if __name__ == "__main__":
    main()
