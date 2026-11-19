# Inkboard cloud

The collaboration backend for Inkboard team boards, entirely on Cloudflare:

| Piece | What it holds |
|---|---|
| Worker (`src/index.ts`) | HTTP API, Google sign-in, permissions, WebSocket routing |
| Durable Object `BoardRoom` (`src/room.ts`), one per board | The board's shapes in SQLite, live edits, cursors and presence |
| D1 `inkboard-db` (`migrations/`) | Accounts, sessions, boards, members |
| R2 `inkboard-assets` | Board images, named by SHA-256 |

Every device keeps its own copy of a board. A room gives each change the next revision number, stores it and sends it to everyone else; the last change to a shape wins. Presence (cursor, selection, page, tool, view) is relayed but never stored. Each stored shape remembers who created it and who changed it last.

## Sign-in

Google Workspace accounts from `ALLOWED_DOMAINS` only, using Google's OAuth device flow, so TVs, phones and desktops sign in the same way: the app shows a code and a QR link, the person approves in any browser. The server checks the ID token's signature, audience, `email_verified` and `hd` (Workspace domain), then issues its own session token (stored only as a SHA-256 hash, 60 days). The Google client secret and device code never reach the apps.

Roles per board: **owner**, **editor**, **viewer**. Owners invite people by email (inside the allowed domains) and can open a board to the whole organisation for viewing or editing. Changing someone's access disconnects them so they reconnect with the new role; viewers' edits are refused by the room.

## Deploy

1. `npm ci --legacy-peer-deps`
2. `npx wrangler login`
3. `npx wrangler d1 create inkboard-db` and put the printed id in `wrangler.toml`.
4. `npx wrangler r2 bucket create inkboard-assets`
5. In Google Cloud Console → APIs & Services → Credentials, create an OAuth client of type **TVs and Limited Input devices**. Put its client id in `GOOGLE_CLIENT_ID` in `wrangler.toml`, then `npx wrangler secret put GOOGLE_CLIENT_SECRET`. Set the OAuth consent screen to **Internal** so only your Workspace can use it.
6. Set `ALLOWED_DOMAINS` in `wrangler.toml` (comma-separated, e.g. `example.com,example.org`).
7. `npm run db:migrate` then `npm run deploy`.
8. Point the apps at the Worker: `DEFAULT_SERVER` in `rust/core/src/ui/cloud/mod.rs` for builds, or `INKBOARD_SERVER` / the `server` setting for testing.

## Develop

```sh
npm test                        # Worker tests (Miniflare), including live sockets
npx wrangler dev --var ALLOWED_DOMAINS:example.com
```

The Rust apps' end-to-end tests (`two_apps_share_a_team_board`, `two_devices_edit_one_board_live`) run against `wrangler dev`; `.github/workflows/cloud.yml` shows how to seed two test sessions.

## API

All `/api/*` calls except sign-in take `Authorization: Bearer <session token>`.

| Call | |
|---|---|
| `POST /api/auth/device` | Start sign-in → `{handle, userCode, verificationUrl, interval}` |
| `POST /api/auth/device/poll` `{handle}` | `202` pending, `200 {token, user}` |
| `GET /api/me`, `POST /api/auth/logout` | |
| `GET /api/boards`, `POST /api/boards {title}` | Boards you own, were invited to, or your organisation can open |
| `PATCH /api/boards/:id {title?, orgAccess?}`, `DELETE /api/boards/:id` | |
| `GET/PUT /api/boards/:id/members`, `DELETE /api/boards/:id/members/:email` | |
| `GET /api/boards/:id/socket` | WebSocket into the board's room; protocol at the top of `src/room.ts` |
| `PUT/GET /api/assets/:sha256` | Images up to 32 MB |
