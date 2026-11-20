# RoALT online counter

The Cloudflare Worker behind the "N using RoALT now" count beside the Discord button.

Each running RoALT with **Settings > About > Show how many people use RoALT** on sends
`POST /ping` with `{"id":"<random UUID made once per launch>","playing":<running Roblox clients>}`
every 2 minutes. The Worker answers `{"online": N, "playing": M}`: how many different ids pinged in
the last 5 minutes, and their clients added up (each id counts for at most 100). Nothing else is
sent, and the Worker keeps the ids in memory only. With the setting off, RoALT sends nothing.

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

The free plan allows 100,000 requests a day: about 140 people online at once with 2-minute pings.
Past that, Workers Paid ($5 a month) allows 10 million.
