# TimeFighter hall of fame (Cloudflare Worker)

The online list behind the game's hall of fame: a small Cloudflare Worker
with one KV namespace. The game works without it; scores then stay in the
player's browser.

- `POST /scores` takes a finished round (name, mode, level, score,
  duration, breakdown), **recomputes the score from the breakdown** with the
  game's rules and checks it against the mission (`limits.json`: enemies by
  class, missiles, par time; a victory needs every kill; at least 15 s + 3 s
  per kill). Accepted scores go on the all-time list and the week's list.
- `GET /scores?mode=hunt&level=normal&period=all|week&limit=10` returns the
  best entries (at most 100 are kept per list).
- Stored per score: pilot name, score, date (mode and level are in the key).
  **No IP address is stored.** The rate limit (one score per minute per
  address) keeps a salted hash of the address for 60 seconds only.
- Names: 3 to 12 characters of A-Z, 0-9, space, `-` and `_`, plus a short
  block list. Custom levels never go online.
- CORS only for `https://timefighter.trendgegner.com` (`ALLOWED_ORIGIN` in
  `wrangler.toml`).

`limits.json` and `test/vectors.json` are written by the game's tests
(`UPDATE_LEADERBOARD=1` with `cargo test score`). A test fails if the game's
missions or rules change and the files were not updated; then update them
and deploy the worker again.

## Tests

Node 20 or newer, no packages needed:

```sh
cd leaderboard
node --test
```

## Setting it up (once)

You need the Cloudflare account that holds the `trendgegner.com` zone.

### 1. Create the KV namespace

Dashboard: **Storage & Databases → KV → Create**, name it
`timefighter-scores`, and copy its **ID**.

Or on the command line: `npx wrangler@4 kv namespace create timefighter-scores`
(prints the ID).

Put the ID into `wrangler.toml` instead of `REPLACE_WITH_KV_NAMESPACE_ID`.

### 2. Deploy the worker

With the command line (simplest, keeps everything in the repo):

```sh
cd leaderboard
npx wrangler@4 login
npx wrangler@4 deploy
```

Optional, for a rate limit salt nobody can guess:
`npx wrangler@4 secret put RATE_SALT` (any long random text).

Or in the dashboard: **Workers & Pages → Create → Worker**, name it
`timefighter-leaderboard`, paste `src/worker.js`, `src/score.js`,
`src/names.js` and `limits.json` (keeping the folder layout), then under
**Settings → Bindings** add the KV namespace with the variable name
`SCORES`, and under **Settings → Variables** add
`ALLOWED_ORIGIN = https://timefighter.trendgegner.com`.

### 3. Custom domain

`wrangler.toml` already asks for `api.timefighter.trendgegner.com`; with
`wrangler deploy` Cloudflare creates the DNS record and the certificate.
In the dashboard: **Worker → Settings → Domains & Routes → Add → Custom
domain** → `api.timefighter.trendgegner.com`.

Check: `https://api.timefighter.trendgegner.com/scores?mode=hunt&level=normal`
answers `{"entries":[]}`.

### 4. Switch the game on

In `web/index.html` set

```html
<meta name="tf-leaderboard" content="https://api.timefighter.trendgegner.com" />
```

and deploy the game as usual. Without this address (or with it empty) the
game keeps scores on the device only.

## Maintenance

- Clear a list: delete its key (`all:<mode>:<level>`) in the KV namespace.
  Weekly lists (`week:<yyyy>-W<ww>:<mode>:<level>`) expire after 21 days.
- Remove a single entry: edit the key's JSON in the dashboard.
- Free plan limits (1,000 KV writes a day) allow about 300 scores a day;
  each score writes three keys.
