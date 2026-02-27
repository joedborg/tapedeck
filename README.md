# 📼 Tapedeck

A self-hosted BBC iPlayer & BBC Sounds download manager.
**Rust/Axum** backend · **Ember.js** web UI · **SQLite** persistence · **Docker** deployment.

[!WARNING]
A UK TV licence is required to access BBC iPlayer TV content legally.

---

## Features

- **Search** BBC iPlayer and BBC Sounds programmes (TV and radio) via `get_iplayer`
- **Series drill-down** — expand any show on the search page to browse all series and episodes; queue an entire series or individual episodes
- **Queue management** — add, remove, cancel, retry, reorder downloads
- **Background worker pool** — configurable concurrent downloads
- **Exponential-backoff retries** — automatically retry failed downloads up to a configurable limit (2 s → 4 s → 8 s …)
- **Scheduled downloads** — specify a future date/time per item
- **Live progress** — WebSocket push updates (progress bar, speed, ETA)
- **Basic auth** — token-based login, multi-user support
- **Settings UI** — configure output directory, quality, tools, proxy, concurrency and retry limits

---

## Quick start (Docker)

```bash
# 1. Clone
git clone https://github.com/you/tapedeck && cd tapedeck

# 2. Configure
cp .env.example .env
#   → Edit .env: set TAPEDECK_SECRET, ADMIN_PASSWORD, DOWNLOAD_DIR

# 3. Run
docker compose up -d

# 4. Open
open http://localhost:3000
```

Log in with the `ADMIN_USERNAME` / `ADMIN_PASSWORD` you set in `.env`
(defaults: `admin` / `changeme`).

---

## Configuration

All settings come from environment variables (or a `.env` file — copy `.env.example` to get started):

| Variable               | Default                      | Description                                                                                      |
| ---------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------ |
| `TAPEDECK_SECRET`      | _(required in prod)_         | HMAC signing secret — generate with `openssl rand -hex 32`                                       |
| `ADMIN_USERNAME`       | `admin`                      | Initial admin username, seeded on first boot                                                     |
| `ADMIN_PASSWORD`       | `changeme`                   | Initial admin password, seeded on first boot                                                     |
| `DOWNLOAD_DIR`         | `./downloads`                | Host directory where downloaded programmes are stored (mounted as `/downloads` in the container) |
| `MAX_CONCURRENT`       | `5`                          | Maximum simultaneous downloads                                                                   |
| `MAX_DOWNLOAD_RETRIES` | `5`                          | Times to retry a failed download (0 = no retries); backs off exponentially (2 s → 4 s → 8 s …)   |
| `PROXY`                | _(empty)_                    | Optional HTTP proxy URL passed to `get_iplayer`                                                  |
| `BIND`                 | `0.0.0.0:3000`               | HTTP listen address                                                                              |
| `DATABASE_URL`         | `/data/tapedeck.db`          | SQLite path inside the container                                                                 |
| `OUTPUT_DIR`           | `/downloads`                 | Download destination inside the container                                                        |
| `GET_IPLAYER_PATH`     | `/usr/local/bin/get_iplayer` | Path to the `get_iplayer` binary                                                                 |
| `FFMPEG_PATH`          | `/usr/bin/ffmpeg`            | Path to `ffmpeg`                                                                                 |

Runtime settings (output dir, quality, retry limit, concurrency) can also be updated via the **Settings** page in the UI and are stored in the database; they take effect on the next download attempt.

### Quality values

The **Default Quality** setting accepts the following values:

| Value   | TV (`--tv-quality`) | Radio (`--radio-quality`) |
| ------- | ------------------- | ------------------------- |
| `best`  | `fhd`               | `high`                    |
| `good`  | `hd`                | `standard`                |
| `worst` | `mobile`            | `low`                     |

Raw get_iplayer values (`1080p`, `720p`, `sd`, etc.) can also be entered directly.

---

## Development

### Backend (Rust)

```bash
cd backend
cargo run
# binds to http://localhost:3000
# API at http://localhost:3000/api
# WS  at ws://localhost:3000/ws?token=<token>
```

Requires `get_iplayer` (v3.36+) and `ffmpeg` to be on `PATH`.

### Frontend (Ember)

```bash
cd frontend
npm install
npm start   # dev server on http://localhost:4200 (proxies /api → localhost:3000)
```

Production build output goes to `frontend/dist/`, which is served as static files by the Rust server.

---

## REST API

| Method   | Path                                       | Description                              |
| -------- | ------------------------------------------ | ---------------------------------------- |
| `POST`   | `/api/auth/login`                          | Login → `{ token, user_id, username }`   |
| `GET`    | `/api/queue`                               | List queue; `?status=&page=&per_page=`   |
| `POST`   | `/api/queue`                               | Add item                                 |
| `GET`    | `/api/queue/:id`                           | Get item                                 |
| `DELETE` | `/api/queue/:id`                           | Cancel / remove                          |
| `POST`   | `/api/queue/:id/retry`                     | Retry failed/cancelled                   |
| `POST`   | `/api/queue/reorder`                       | Bulk reprioritise                        |
| `GET`    | `/api/search?q=&type=tv\|radio`            | Search programmes                        |
| `GET`    | `/api/search/episodes?pid=&type=tv\|radio` | List all episodes for a brand/series PID |
| `POST`   | `/api/search/refresh`                      | Refresh programme cache                  |
| `GET`    | `/api/settings`                            | List all settings                        |
| `PUT`    | `/api/settings/:key`                       | Update one setting                       |
| `PATCH`  | `/api/settings`                            | Bulk update settings                     |
| `GET`    | `/api/users`                               | List users                               |
| `GET`    | `/api/users/me`                            | Current user                             |
| `POST`   | `/api/users`                               | Create user                              |
| `DELETE` | `/api/users/:id`                           | Delete user                              |
| `PUT`    | `/api/users/:id/password`                  | Change password                          |

All endpoints except `/api/auth/login` require `Authorization: Bearer <token>`.

### WebSocket

Connect to `ws://<host>/ws?token=<token>` for real-time events:

```jsonc
// Progress update (HLS streams — live percent + speed + ETA)
{ "type": "progress", "id": "...", "progress": 42.5, "speed": "97.8 Mb/s", "eta": "00:01:23" }
// Progress update (DASH streams — no percent available; shows speed/elapsed)
{ "type": "progress", "id": "...", "progress": 0, "speed": "2.3x", "eta": "00:05:32" }
// Status change
{ "type": "status_change", "id": "...", "status": "done" }
// Item added / removed
{ "type": "item_added",   "item": { ... } }
{ "type": "item_removed", "id": "..." }
// Error
{ "type": "error", "id": "...", "message": "..." }
```

For DASH streams (`progress: 0`), the UI shows an **indeterminate animated bar** until the download completes. A heartbeat event is also emitted every 30 seconds with the elapsed time so the UI stays live.

---

## Project layout

```
tapedeck/
├── backend/               Rust/Axum service
│   ├── src/
│   │   ├── main.rs        Entry point
│   │   ├── config.rs      Environment config (incl. MAX_DOWNLOAD_RETRIES)
│   │   ├── auth.rs        Password hashing + token auth
│   │   ├── db.rs          SQLite pool + migrations
│   │   ├── models.rs      Shared types + DTOs
│   │   ├── queue.rs       Background worker pool + retry logic
│   │   ├── iplayer.rs     get_iplayer subprocess wrapper + episode parser
│   │   ├── state.rs       Shared Axum state
│   │   └── routes/
│   │       ├── mod.rs     Router assembly
│   │       ├── queue.rs   Queue endpoints
│   │       ├── search.rs  Search + episode-listing endpoints
│   │       ├── settings.rs Settings CRUD
│   │       ├── users.rs   User management
│   │       └── ws.rs      WebSocket handler
│   └── migrations/
│       ├── 001_initial.sql
│       └── 002_max_retries_setting.sql
├── frontend/              Ember.js SPA
│   ├── app/
│   │   ├── app.js
│   │   ├── router.js
│   │   ├── services/
│   │   │   ├── api.js     HTTP client (incl. fetchEpisodes)
│   │   │   └── socket.js  WebSocket client
│   │   ├── routes/        Route classes
│   │   ├── controllers/   Controller classes (search: series drill-down)
│   │   ├── helpers/
│   │   │   └── includes.js
│   │   ├── templates/     Handlebars templates
│   │   └── styles/
│   │       └── app.css
│   └── config/
├── Dockerfile
├── docker-compose.yml
└── .env.example
```

---

## Licence

[GNU General Public License v3.0 or later](LICENSE) (GPL-3.0-or-later)
