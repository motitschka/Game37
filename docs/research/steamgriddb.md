# SteamGridDB API v2: artwork fetch for Game covers and heroes

Research for fetching a tall cover ("grid") and a wide "hero" image per Game, matching on cleaned-up
title, auto-picking the top result with a picker to browse others. Game37's network policy only allows
the libretro build server and SteamGridDB, so §6 (hosts) matters most for that allowlist.

The official docs page (`https://www.steamgriddb.com/api/v2`) and the terms page returned connection
failures from every fetch tool available here (`WebFetch`, `curl`) — the one WebFetch attempt that did
return content came back empty/loading-only. All API shape facts below are therefore cross-checked
across the official client libraries (`github.com/SteamGridDB/node-steamgriddb`,
`github.com/SteamGridDB/java-steamgriddb`) and, where those were silent, a well-known third-party Rust
client (`github.com/PhilipK/steamgriddb_api`) and Go client (`github.com/boppreh/steamgrid`), which agree
with each other everywhere they overlap.

Confidence key: **documented** (an official SteamGridDB-owned library's README/source, or the ToS text
itself, fetched this session) or **inferred from a library's source** (a third-party client's source
code or a community example, not from SteamGridDB directly — lower confidence, flagged).

## 1. Base URL and authentication

Base URL: `https://www.steamgriddb.com/api/v2` — **documented**, default `baseURL` in
[node-steamgriddb's README](https://github.com/SteamGridDB/node-steamgriddb) and constructor example
(`new SGDB({..., baseURL: 'https://www.steamgriddb.com/api/v2'})`). The Java client hardcodes
`https://www.steamgriddb.com/API/v2/` (capital `API`) in
[`SGDBConnectionManager.java`](https://github.com/SteamGridDB/java-steamgriddb/blob/master/src/main/java/com/steamgriddb/Connection/SGDBConnectionManager.java) — likely harmless case-insensitivity on the server, but use lowercase `api` to match the docs URL and the JS client.

Auth header: `Authorization: Bearer <api_key>` — **documented**, set in
[node-steamgriddb `src/index.ts`](https://github.com/SteamGridDB/node-steamgriddb/blob/master/src/index.ts)
(`this.headers.Authorization = \`Bearer ${this.key}\``) and in Java's `SGDBConnectionManager`
(`.setHeader("Authorization", "Bearer " + authKey)`). Keys are created on
`https://www.steamgriddb.com/profile/preferences` (documented in both READMEs). Per the task, no key was
used or looked for here.

## 2. Search by name (autocomplete)

`GET /search/autocomplete/{urlencoded_term}` — **documented**, from
[node-steamgriddb `searchGame()`](https://github.com/SteamGridDB/node-steamgriddb/blob/master/src/index.ts)
and Java's `Search.searchGamesByName()`.

Response (wrapped — see §5): `data` is an array of:

| field | type | source |
|---|---|---|
| `id` | number | README example JSON |
| `name` | string | README example JSON |
| `types` | string[] (platform tags, e.g. `"steam"`) | README example JSON |
| `verified` | boolean | README example JSON |
| `release_date` | number (unix timestamp, presumed) | `SGDBGame` interface in `src/index.ts` only — not shown in the example JSON, so its exact shape is **inferred from a library's source**, unverified |

Example from the [README](https://github.com/SteamGridDB/node-steamgriddb):
```json
[{ "id": 2254, "name": "Half-Life 2", "types": ["steam"], "verified": true }]
```
Ordering: **not documented anywhere found**. No source states a sort key; every example and every
library just takes `data[0]` as "the" match, implying the server ranks the best match first, but this is
an assumption, not a confirmed rule. Flag as unverified — the app should treat the top result as "most
likely," not "guaranteed best."

## 3 & 4. Grids and heroes for a game id

`GET /grids/{idType}/{id}` and `GET /heroes/{idType}/{id}` — **documented** path shape from
node-steamgriddb (`/grids/${options.type}/${options.id}`, `/heroes/${options.type}/${options.id}`) and
Java's `Grid.java` (`grids/game/`, `grids/steam/`, `grids/egs/`, `grids/uplay/`, `grids/origin/`,
`grids/gog/`). `idType` is `game` for a SteamGridDB game id (what §2's search returns) or a store id type:
`steam`, `origin`, `egs`, `uplay`, `gog`, and (per the Rust client's `Platform` enum, **inferred**)
`bnet`, `flashpoint`. Game37 only needs `game`.

Query parameters, all **inferred from a library's source** — `node-steamgriddb`'s
[`buildQuery()`](https://github.com/SteamGridDB/node-steamgriddb/blob/master/src/index.ts) documents the
parameter *names* (official, but gives no allowed-value lists); the Rust and Go clients below fill in the
values:

| param | values | source |
|---|---|---|
| `styles` (comma list) | `alternate`, `blurred`, `white_logo`, `material`, `no_logo` | Rust `Style` enum in [`query_parameters.rs`](https://github.com/PhilipK/steamgriddb_api/blob/main/src/query_parameters.rs); same 5 values in [boppreh/steamgrid](https://github.com/boppreh/steamgrid)'s `-styles` flag comment |
| `dimensions` (comma list) — grids | `460x215`, `920x430`, `600x900`, `342x482`, `660x930`, `512x512`, `1024x1024` | Rust `GridDimentions` enum; boppreh/steamgrid's `-coverdimensions` (`600x900,342x482,660x930`) and `-bannerdimensions` (`460x215,920x430`) defaults agree |
| `dimensions` — heroes | `1920x620`, `3840x1240`, `1600x650` | Rust `HeroDimentions` enum; boppreh/steamgrid's `-herodimensions` default (`1920x620,3840x1240,1600x650`) agrees |
| `mimes` (comma list) | `image/png`, `image/jpeg`, `image/webp` | Rust `MimeType` enum |
| `types` (comma list) | `static`, `animated` | Rust `AnimtionType` enum; boppreh/steamgrid's `-types` flag |
| `nsfw` | `true` \| `false` \| `any` | Rust `Nsfw` enum; boppreh/steamgrid's `-nsfw` flag |
| `humor` | `true` \| `false` \| `any` | Rust `Humor` enum; boppreh/steamgrid's `-humor` flag |
| `epilepsy` | name only confirmed; values not independently found (presumed `true`\|`false`\|`any` by analogy) | param *name* is **documented** — it's in node-steamgriddb's `singleParams` list in `buildQuery()`; the Rust/Go clients don't implement it at all, so its values are unverified |
| `oneoftag`, `page` | tag id; page number | param *names* **documented** the same way (node-steamgriddb `singleParams`); no value constraints found anywhere |

Response (wrapped — see §5): `data` is an array of image objects. Field list reconciled from
node-steamgriddb's README example JSON + its `SGDBImage` TS interface (**documented**) and the Rust
`Image` struct in [`images.rs`](https://github.com/PhilipK/steamgriddb_api/blob/main/src/images.rs)
(**inferred**, but it independently lists the same fields plus a few node's interface omits):

| field | type |
|---|---|
| `id` | number |
| `score` | number |
| `style` | string (e.g. `"blurred"`) |
| `url` | string (full-size image URL) |
| `thumb` | string (thumbnail URL) |
| `tags` | string[] |
| `author` | `{ name: string, steam64: string, avatar: string }` |
| `width`, `height` | number |
| `nsfw`, `humor`, `epilepsy`, `lock` | boolean |
| `notes` | string or null |
| `mime` | string (e.g. `"image/png"`) |
| `language` | string |
| `upvotes`, `downvotes` | number |

One inconsistency worth flagging: node-steamgriddb's own TS interface declares `style: string[]`, but its
own README example JSON for that same field shows a single string (`"style": "blurred"`), and the Rust
struct also types it as a single `StyleType`, not an array. Treat `style` as a single string; the array
type in the TS interface looks like a typo in the library, not a real API shape.

## 5. Pagination, rate limits, errors

Response envelope — **inferred from a library's source**, but the only parts needed (`success`, `data`,
`errors`) are also exercised in node-steamgriddb's own error-handling code, so those three are effectively
documented by the official library even without a docs-page quote:
```json
{ "success": true, "data": [ /* ... */ ] }
{ "success": false, "status": 404, "errors": ["Game not found."] }
```
Shape confirmed by the Rust client's `Response<T>` / `SteamGridDbError` structs
([`response.rs`](https://github.com/PhilipK/steamgriddb_api/blob/main/src/response.rs)) and its test
fixtures, which show `status: 404, errors: ["Game not found"]` for a bad game id and
`errors: ["Asset does not exist"]` for a bad asset id. node-steamgriddb's README shows the same shape from
the live API: a 404 with `"Game not found."` for `getGrids` on game id `0`, and a 403 with
`"This grid isn't yours."` for deleting someone else's grid — both **documented** (quoted directly from
the README).

401 for a missing/bad key was **not found in any source** — no library or example triggers it. Standard
REST practice suggests 401, but this is unverified; treat it as a guess.

Pagination: `page` is a real, documented parameter name (§3/4), but no source found states the page size
or a `limit` parameter — the API may use a fixed page size. **Unverified**, could not confirm from a
primary source.

Rate limits: several community threads (SteamTinkerLaunch's
[wiki page](https://github.com/frostworx/steamtinkerlaunch/wiki/SteamGridDB)) acknowledge "you may be
rate-limited by the SteamGridDB API" when querying many games, implying 429 responses exist, but no
numeric threshold (requests/minute, `Retry-After` header, etc.) was found anywhere. **Unverified** —
build in backoff-on-429 defensively rather than relying on a documented number.

## 6. Hosts the image URLs point at

Two different hosts show up depending on how old the example is:

- `s3.amazonaws.com` (path-style, bucket `steamgriddb`, e.g.
  `https://s3.amazonaws.com/steamgriddb/grid/f033ab37c30201f73f142449d037028d.png`) — **documented**,
  verbatim from [node-steamgriddb's README](https://github.com/SteamGridDB/node-steamgriddb) example
  response. This example is old (Half-Life 2 grid from years ago) and may be stale.
- `cdn2.steamgriddb.com` (e.g. `.../file/sgdb-cdn/thumb/d8581dcfb050f8f1773768110b80e9e4.jpg`) —
  **inferred**, a real URL pasted by a user in a
  [Heroic Games Launcher issue](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/2607)
  from 2023, much more recent than the README's S3 example.

Author avatars (`author.avatar`, not an artwork field Game37 needs) point at Steam's own CDN
(`steamcdn-a.akamaihd.net` in the old README example, newer Steam CDNs differ) — not relevant to the
network allowlist since Game37 doesn't need to fetch avatars.

**For the network policy**: allowlist `cdn2.steamgriddb.com` as the live CDN, and keep `s3.amazonaws.com`
allowed too since it's the one directly documented example and old images may still resolve there. Since
neither host could be confirmed from the live API (unreachable this session), spot-check the actual
`url`/`thumb` values returned by a real search once a key is available, before locking the allowlist down
to an exact host list.

## 7. Terms of use

Fetched the terms page content this session (`https://www.steamgriddb.com/terms`) — **documented**,
quoted verbatim:

- Personal use only: "access the Service for any reason other than your personal, non-commercial use
  solely as permitted by the normal functionality of the Service" is prohibited.
- No bulk/automated load: prohibits "any automated use of the Site, the Service or the related systems,
  or take any action that we deem to impose or to potentially impose an unreasonable or disproportionately
  large load on our servers."
- No redistribution: prohibits "distribute any part or parts of the Site or the Service without our
  explicit written permission," with a carve-out only for search-engine spiders.
- No commercial use: prohibits using the Service "for the solicitation of business in the course of trade
  or in connection with a commercial enterprise."
- Nothing explicit about caching fetched images locally, and no explicit attribution requirement for API
  consumers. Game37 showing/caching a chosen cover and hero for a user's own personal library plausibly
  falls under "normal functionality of the Service" (that's exactly what the site itself does), but there
  is no written caching/retention policy to point to — this is a gap in the ToS, not a confirmed
  allowance. Crediting `author.name` (already returned by the API) costs nothing and isn't prohibited.

## 8. Suggested request sequences

**Top cover + top hero for title X:**
1. `GET /search/autocomplete/{urlencode(X)}` → take `data[0].id` as the game id (top result; ordering is
   unverified, see §2).
2. `GET /grids/game/{id}?dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false` → take
   `data[0]` (highest `score` first is an assumption from every example seen, not a documented guarantee).
3. `GET /heroes/game/{id}?dimensions=1920x620,3840x1240,1600x650&types=static&nsfw=false&humor=false` →
   take `data[0]`.

**List more covers for the picker:**
- Same `GET /grids/game/{id}` call as step 2, without narrowing to `data[0]` — render every `data[n].thumb`
  in the picker grid, and fetch `data[n].url` only for the one the user picks. Drop the `dimensions` filter
  if the picker should show every style/size, not just the ones matching the app's target aspect ratio.
  Add `&page=1`, `&page=2`, … if the result set is large (page-size is unverified, see §5 — treat any
  short page as possibly-not-the-last until confirmed).
