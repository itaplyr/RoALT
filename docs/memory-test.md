# Memory test

RoALT's memory settings have never been measured on a real client. This test gives the numbers that decide the default trim interval, whether trim is worth keeping on, and which settings to recommend.

## Setup
- One alt account, one game, the same place each run, standing still.
- Let the client settle for 3 minutes before each reading.
- Note the PC's RAM, GPU, Windows version and pagefile size.

## What to record
For each state, take these from RoALT's Monitor tab or Task Manager's Details tab:
- Memory (private working set)
- Commit size
- Dedicated GPU memory
- `dwm.exe` memory, from Task Manager

## States

| # | State | Memory | Commit | GPU memory | `dwm.exe` |
|---|---|---|---|---|---|
| 1 | Visible, normal size, Lowest graphics and Lighter rendering off | | | | |
| 2 | Visible, normal size, Lowest graphics on | | | | |
| 3 | Visible, normal size, Lowest graphics and Lighter rendering on | | | | |
| 4 | Visible, tiny (240x180) | | | | |
| 5 | Hidden, Live previews on, Home open | | | | |
| 6 | Hidden, Live previews off, asleep | | | | |
| 7 | Same as 3, in a second game | | | | |

States 1 to 3 need a relaunch after each settings change. Graphics are only read at launch.

## Trim
RoALT logs each trim in `roalt.log`:

```text
trim of client 1234 (by hand): before: working set ... MB, private working set ... MB, commit ... MB; right after: ...
trim of client 1234 (by hand): 10 s later: ...
trim of client 1234 (by hand): 60 s later: ...
trim of client 1234 (by hand): 300 s later: ...
```

Use **Page memory out now** on Home's client menu, wait 5 minutes, and keep those four lines. Do it for:
1. The client visible, no CPU limit.
2. The client visible, CPU limit 10%.
3. The client hidden and asleep.

While the client is limited, also note if it stutters, or if disk use climbs in the Monitor tab.

## What to send back
- The filled table.
- The trim lines from `roalt.log`. The diagnostics report includes them.
- Whether any client crashed or disconnected during the run.

## Reading the results
- **Trim:** if Memory is back near its old value within 60 s, the trim saved nothing lasting.
- **Commit:** it should stay flat through a trim. If it doesn't, the code's assumption is wrong.
- **Graphics:** states 1 to 3 show which settings are worth their cost.
- **Previews:** states 5 and 6 show what live previews of hidden clients cost.
