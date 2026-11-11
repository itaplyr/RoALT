// Dev-only: every primitive in its states, the island on a fake model, and a live contrast table
// (?gallery). Toggles for theme, accent, RTL and reduced motion. Never in a production build.
import { useEffect, useState, type ReactNode } from "react";
import { accentTokens, contrast } from "../app/theme";
import { Button, IconButton } from "../ui/Button";
import { Chip, Disclosure, Kbd, Skeleton } from "../ui/Bits";
import { OffOrNumberField, NumberField, Segmented, Switch, TextField } from "../ui/controls";
import { ConfirmDialog, Dialog, DialogSteps } from "../ui/Dialog";
import { Checkbox, Input, Radio, Select } from "../ui/Field";
import { Island } from "../ui/island/Island";
import type { IslandModel } from "../ui/island/model";
import { Dial, Meter } from "../ui/Meter";
import { Collapse, IconSwap, ListPresence, Num } from "../ui/motion";
import { PillLabel, PillSeparator, PillToolbar } from "../ui/PillToolbar";
import { MenuCheckItem, MenuItem, MenuRadioItem, MenuSeparator, Popover, useAnchor } from "../ui/Popover";
import { RANGES } from "../ui/ranges";
import { StatusPill, type Tone } from "../ui/Status";
import { Tabs } from "../ui/Tabs";
import { toast } from "../ui/Toast";

const PAIRS: [string, string, number][] = [
  ["--fg", "--ground", 4.5], ["--fg-2", "--ground", 4.5], ["--fg-3", "--ground", 4.5], ["--fg-3", "--surface-1", 4.5], ["--fg-3", "--surface-2", 4.5],
  ["--fg", "--surface-3", 4.5], ["--fg-2", "--surface-3", 4.5], ["--on-accent", "--accent", 4.5], ["--accent", "--island", 3],
  ["--edge", "--ground", 3], ["--edge", "--surface-2", 3], ["--focus", "--ground", 3], ["--ok", "--ground", 4.5], ["--info", "--ground", 4.5],
  ["--attention", "--ground", 4.5], ["--warn", "--ground", 4.5], ["--err", "--ground", 4.5],
];

function hex(v: string) {
  const c = document.createElement("span");
  c.style.color = v;
  document.body.append(c);
  const rgb = getComputedStyle(c).color.match(/\d+/g)!.slice(0, 3).map(Number);
  c.remove();
  return `#${rgb.map((n) => n.toString(16).padStart(2, "0")).join("")}`;
}

function ContrastTable({ tick }: { tick: number }) {
  const css = getComputedStyle(document.documentElement);
  void tick;
  return (
    <table className="text-[13px]">
      <tbody>
        {PAIRS.map(([fg, bg, need]) => {
          const [a, b] = [hex(css.getPropertyValue(fg).trim()), hex(css.getPropertyValue(bg).trim())];
          const r = contrast(a, b);
          return (
            <tr key={fg + bg} style={{ color: r >= need ? "var(--fg-2)" : "var(--err)" }}>
              <td className="pe-4">{fg} on {bg}</td>
              <td className="pe-4 mono">{a} / {b}</td>
              <td className="mono">{r.toFixed(2)} {r >= need ? "ok" : `< ${need}`}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

const Section = ({ title, children }: { title: string; children: ReactNode }) => (
  <section className="panel flex flex-col gap-4 p-5">
    <h2 className="section-title">{title}</h2>
    {children}
  </section>
);

export function Gallery() {
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const [accent, setAccent] = useState("");
  const [rtl, setRtl] = useState(false);
  const [rm, setRm] = useState(false);
  const [tick, setTick] = useState(0);
  const [on, setOn] = useState(true);
  const [seg, setSeg] = useState("b");
  const [tab, setTab] = useState(1);
  const [fps, setFps] = useState(60);
  const [n, setN] = useState(42);
  const [dialog, setDialog] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [step, setStep] = useState(0);
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState([1, 2, 3]);
  const [icon, setIcon] = useState(false);
  const [cpu, setCpu] = useState(0);
  const [current, setCurrent] = useState("home");
  const menu = useAnchor();

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.dir = rtl ? "rtl" : "ltr";
    root.toggleAttribute("data-rm", rm);
    if (accent) {
      const a = accentTokens(accent, theme);
      root.dataset.accent = "";
      root.style.setProperty("--user-accent", a.fill);
      root.style.setProperty("--user-on-accent", a.ink);
      root.style.setProperty("--user-accent-text", a.text);
      root.style.setProperty("--user-accent-ring", a.ring);
    } else delete root.dataset.accent;
    setTick((x) => x + 1);
  }, [theme, accent, rtl, rm]);

  useEffect(() => {
    const t = setInterval(() => setCpu(Math.round(Math.random() * 100)), 1500);
    return () => clearInterval(t);
  }, []);

  const leaf = (id: string, icon: "home" | "play" | "accounts" | "gauge", label: string) => ({ id, icon, label, current: current === id, onSelect: () => setCurrent(id), shortcut: "Ctrl+1" });
  const model: IslandModel = {
    label: "Sections",
    main: [leaf("home", "home", "Home"), leaf("play", "play", "Play"), leaf("accounts", "accounts", "Accounts"), leaf("perf", "gauge", "Performance")],
    mainOrder: { ids: [], onOrder: () => {} },
    groups: [
      { id: "tools", icon: "tools", label: "More tools", children: [
        { id: "servers", icon: "servers", label: "Servers", current: current === "servers", onSelect: () => setCurrent("servers") },
        { id: "monitor", icon: "monitor", label: "Monitor", current: current === "monitor", onSelect: () => setCurrent("monitor") },
        { id: "debug", icon: "terminal", label: "Debug", current: current === "debug", onSelect: () => setCurrent("debug") },
      ] },
      { id: "settings", icon: "settings", label: "Settings", keepIcon: true, children: [
        { id: "s-launch", icon: "rocket", label: "Launching", current: current === "s-launch", onSelect: () => setCurrent("s-launch") },
        { id: "s-look", icon: "palette", label: "Appearance", current: current === "s-look", onSelect: () => setCurrent("s-look") },
      ] },
    ],
  };
  const tones: Tone[] = ["ok", "info", "attention", "warn", "err", "neutral"];

  return (
    <main className="scroll h-full page flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="page-title me-auto">Gallery</h1>
        <Segmented label="Theme" value={theme} onChange={setTheme} options={[{ value: "dark", label: "Dark" }, { value: "light", label: "Light" }]} />
        <Segmented label="Accent" value={accent} onChange={setAccent} options={[{ value: "", label: "Mono" }, { value: "#c6f432", label: "Lime" }, { value: "#ffd23f", label: "Gold" }, { value: "#3b82f6", label: "Blue" }]} />
        <Checkbox label="RTL" checked={rtl} onChange={setRtl} />
        <Checkbox label="Reduced motion" checked={rm} onChange={setRm} />
      </div>

      <Section title="Island">
        <div className="grid place-items-center py-2" style={{ containerType: "inline-size" }}>
          <Island model={model} inflate={false} />
        </div>
      </Section>

      <div className="grid gap-5" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(420px, 1fr))" }}>
        <Section title="Buttons">
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" icon="play">Play with 3</Button>
            <Button>Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="danger" icon="trash">Remove</Button>
            <Button disabled>Disabled</Button>
            <Button busy>Busy</Button>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="md">Medium</Button>
            <Button size="sm" icon="copy">Small</Button>
            <IconButton icon="refresh" label="Refresh" shortcut="F5" />
            <IconButton icon="dots" label="More" />
            <button className="wc" aria-label="Swap" onClick={() => setIcon((x) => !x)}><IconSwap name={icon ? "sun" : "moon"} /></button>
            <Kbd keys="Ctrl+Shift+F12" />
          </div>
        </Section>

        <Section title="Choices">
          <div className="flex flex-wrap items-center gap-3">
            <Switch label="Switch" checked={on} onChange={setOn} />
            <Switch label="Off" checked={false} onChange={() => {}} disabled />
            <Segmented label="Segmented" value={seg} onChange={setSeg} options={[{ value: "a", label: "Off" }, { value: "b", label: "50%" }, { value: "c", label: "25%" }, { value: "d", label: "10%" }]} />
          </div>
          <Tabs label="Tabs" value={tab} onChange={setTab} items={[{ value: 1, label: "Overview" }, { value: 2, label: "Overrides" }, { value: 3, label: "Cookie", count: 1 }]} />
          <div className="flex flex-wrap gap-2">
            <Chip pressed dot="#7AB0FF" count={12}>Farm</Chip>
            <Chip dot="#5FE0C8" count={3}>AFK</Chip>
            <Chip small>Small</Chip>
            <span className="tag">tag</span>
          </div>
          <div className="flex flex-wrap gap-4">
            <Checkbox label="Checkbox" checked onChange={() => {}} />
            <Checkbox label="Mixed" checked={false} indeterminate onChange={() => {}} />
            <Radio label="Radio" name="r" value="1" checked onChange={() => {}} />
            <Radio label="Other" name="r" value="2" checked={false} onChange={() => {}} />
          </div>
        </Section>

        <Section title="Fields">
          <Input leading="search" placeholder="Filter by name, @user, id" />
          <div className="flex flex-wrap items-center gap-3">
            <TextField label="Launch arguments" value="" placeholder="none" onCommit={async () => {}} className="w-56" />
            <NumberField unit="seconds" short="s" min={2} max={60} value={6} onCommit={() => {}} />
            <Select value="x1" onChange={() => {}} options={[{ value: "x1", label: "x1" }, { value: "x4", label: "x4" }]} />
          </div>
          <div className="flex items-center justify-between">
            <span>FPS cap</span>
            <OffOrNumberField label="FPS cap" value={fps} range={RANGES.fpsCap} unit="FPS" short="FPS" onCommit={setFps} />
          </div>
          <input className="field" aria-invalid="true" defaultValue="Invalid" />
          <input type="range" className="range" min={0} max={100} defaultValue={40} />
        </Section>

        <Section title="Status and data">
          <div className="flex flex-wrap gap-2">
            {tones.map((t) => <StatusPill key={t} tone={t} label={t} pulse={t === "info"} />)}
          </div>
          <div className="flex items-center gap-6">
            <Dial value={cpu} label="CPU" />
            <Dial value={null} label="GPU" />
            <span className="text-[28px] font-semibold"><Num value={cpu} format={(v) => `${Math.round(v)}%`} /></span>
            <div className="flex-1 flex flex-col gap-2">
              <Meter value={cpu} label="CPU" />
              <Meter value={70} label="Memory" tone="attention" />
            </div>
          </div>
          <div className="flex items-center gap-3">
            <Skeleton width={120} height={14} />
            <Skeleton width={60} height={14} />
            <div className="live-bar flex-1" />
          </div>
          <p className="alert">Roblox refuses joins for this game right now.</p>
        </Section>

        <Section title="Overlays">
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => setDialog(true)}>Dialog with steps</Button>
            <Button onClick={() => setConfirm(true)}>Confirm</Button>
            <Button onClick={() => toast("Launched Alt 01.")}>Toast</Button>
            <Button onClick={() => toast("Couldn't reach Roblox.", "error")}>Error toast</Button>
            <button className="btn" popoverTarget="g-menu" style={menu.style}>Menu</button>
          </div>
          <Popover id="g-menu" anchor={menu.name} role="menu" label="Menu">
            <MenuItem icon="play" kbd="Enter" onSelect={() => {}}>Show</MenuItem>
            <MenuItem icon="minimize" onSelect={() => {}}>Minimize</MenuItem>
            <MenuSeparator />
            <MenuCheckItem checked onChange={() => {}}>Efficiency mode</MenuCheckItem>
            <MenuRadioItem checked onSelect={() => {}}>CPU 25%</MenuRadioItem>
            <MenuSeparator />
            <MenuItem icon="x" danger onSelect={() => {}}>Force close</MenuItem>
          </Popover>
          <Disclosure summary="More about anti-AFK">So Roblox doesn't disconnect idle clients after 20 minutes, each client in a game gets a key press.</Disclosure>
          <div>
            <Button size="md" onClick={() => setOpen((o) => !o)}>{open ? "Collapse" : "Expand"}</Button>
            <Collapse open={open}>
              <p className="pt-3 text-fg-2">This row appears because a switch above it is on; it opens to its height on a spring.</p>
            </Collapse>
          </div>
        </Section>

        <Section title="Lists and toolbars">
          <div className="flex gap-2">
            <Button size="md" icon="plus" onClick={() => setItems((l) => [...l, (l.at(-1) ?? 0) + 1])}>Add</Button>
            <Button size="md" icon="trash" onClick={() => setItems((l) => l.slice(1))}>Remove first</Button>
          </div>
          <ListPresence items={items} getKey={(i) => i} className="flex flex-col gap-1" render={(i) => <div className="well px-3 py-2">Client {i}</div>} />
          <PillToolbar label="Selection">
            <PillLabel>3 selected</PillLabel>
            <PillSeparator />
            <Button size="md" variant="ghost" icon="play">Play…</Button>
            <Button size="md" variant="ghost" icon="refresh">Check</Button>
            <Button size="md" variant="ghost" icon="trash">Remove</Button>
            <IconButton icon="x" label="Clear" />
          </PillToolbar>
          <div className="flex items-center gap-2"><span>Count</span><Button size="sm" onClick={() => setN((x) => x + 7)}>+7</Button><span className="text-xl"><Num value={n} /></span></div>
        </Section>

        <Section title="Contrast">
          <ContrastTable tick={tick} />
        </Section>
      </div>

      <Dialog open={dialog} onClose={() => setDialog(false)} title="Export cookies">
        <DialogSteps step={step}>
          {step === 0 ? (
            <div className="dlg-body flex flex-col gap-3">
              <p className="text-fg-2">Anyone with this file can sign in as these 3 accounts.</p>
              <div className="flex justify-end gap-2"><Button onClick={() => setDialog(false)}>Cancel</Button><Button variant="primary" data-autofocus onClick={() => setStep(1)}>Continue</Button></div>
            </div>
          ) : step === 1 ? (
            <div className="dlg-body flex flex-col gap-3">
              <label className="flex flex-col gap-1.5"><span className="label">Master password</span><input type="password" className="field" data-autofocus /></label>
              <div className="flex justify-end gap-2"><Button onClick={() => setStep(0)}>Back</Button><Button variant="primary" onClick={() => setStep(2)}>Export</Button></div>
            </div>
          ) : (
            <div className="dlg-body flex flex-col gap-3">
              <p>Cookies exported: 3.</p>
              <input className="field mono" readOnly value="C:\Users\you\Desktop\roalt-cookies.txt" />
              <p className="text-fg-3 text-[13px]">RoALT deletes the file at 22:40.</p>
              <div className="flex flex-wrap justify-end gap-2"><Button icon="copy">Copy path</Button><Button variant="danger">Delete now</Button><Button variant="primary" data-autofocus onClick={() => { setDialog(false); setStep(0); }}>Done</Button></div>
            </div>
          )}
        </DialogSteps>
      </Dialog>
      <ConfirmDialog open={confirm} onClose={() => setConfirm(false)} onConfirm={() => setConfirm(false)} title="Remove 3 accounts?" text="Their cookies are deleted from this PC." confirmLabel="Remove 3 accounts" />
    </main>
  );
}
