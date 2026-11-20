# RoALT online counter

The Cloudflare Worker behind the "N using RoALT now" count beside the Discord button.

Each running RoALT with **Settings > About > Show how many people use RoALT** on sends
`POST /ping` with `{"id":"<random UUID made once per launch>","playing":<running Roblox clients>}`.
The Worker answers `{"online": N, "playing": M, "every": S}`: how many different ids pinged in the
last 12 minutes, their clients added up (each id counts for at most 100), and how many seconds RoALT
waits before the next ping (`EVERY_S` in `worker.js`, 300 now; RoALT keeps it within 1 to 60
minutes). RoALT 1.1.0 and older ignore `every` and ping every 2 minutes. Nothing else is sent, and
the Worker keeps the ids in memory only. With the setting off, RoALT sends nothing.

To cut requests further, raise `EVERY_S` and deploy; `TTL_MS` follows it. When `Counter`'s code
changes, bump the name in `idFromName` too (a running instance keeps its old code). Workers Logs are
off in `wrangler.toml`: every ping was an observability event.

## Deploy

```
cd server/online-counter
npx wrangler login
npx wrangler deploy
```

`deploy` prints the Worker's `https://roalt-online.<account>.workers.dev` address. Put it in
`ONLINE_URL` in `src-tauri/src/online.rs`.

## Try it locally

```
npx wrangler dev
curl -X POST localhost:8787/ping -d "{\"id\":\"9b2f6c1e-3d4a-4b5c-8d6e-7f8091a2b3c4\",\"playing\":3}"
```

Requests a month are about the people online at once × 43,200 / `EVERY_S`: 745 online at 5-minute
pings is about 107,000 a day, 3.2 million a month (Workers Paid includes 10 million).
