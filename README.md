# Statesu

A blog for your website, without rebuilding your website. Write in Markdown, publish from your terminal or the dashboard, and show posts on your existing site with an embed script or the public API.

- **Hosted** at [statesu.com](https://statesu.com) — open the dashboard and start writing.
- **Self-hosted** — one Rust binary and a SQLite file: email-verified accounts, sessions and personal access tokens, media images on R2 (the server's bucket or your own), per-blog CORS for public delivery. The dashboard and docs are embedded into the binary at compile time.

The server is the source of truth: no Git, no local post storage. Management routes (session or token) and public delivery (`/public/...`, per-blog CORS) are cleanly separated.

User docs: [CLI](https://statesu.com/docs/cli) · [API](https://statesu.com/docs/api) · [Web integration](https://statesu.com/docs/integration).

## Requirements

| For | Needs |
|---|---|
| Backend & CLI | Rust (toolchain pinned in `rust-toolchain.toml`; rustup picks it up automatically). A Resend account for verification mail. |
| Dashboard build | Node.js (any recent version; `npm ci`). |
| Docker deployment | Docker with Docker Compose. |

## Quick start (development)

```sh
cp .env.example .env    # then fill in the values
make dev                # API on 127.0.0.1:8080 + Vite dashboard on :5173
```

`make dev` starts the Rust API in the background and runs the Vite dev server with HMR, proxying `/auth`, `/blogs` and `/public` to the API. Register through the dashboard (`http://localhost:5173`) and confirm the email.

CLI development is separate and does not start the API or web server:

```sh
make dev-cli                                      # show CLI help
make dev-cli ARGS="login"
make dev-cli ARGS="blog list"
```

To use a local server, run `make dev` in another terminal and pass `--server http://localhost:8080` to the CLI.

## Configuration (server)

Copy `.env.example` to `.env`. Environment variables always win over the file; the file is optional in production. Secrets are 32-byte base64 values: `openssl rand -base64 32`.

### Required

| Variable | Description |
|---|---|
| `STATESU_EMAIL_KEY` | AES-256-GCM key encrypting emails at rest. Exactly 32 bytes, base64. |
| `STATESU_EMAIL_HMAC_KEY` | HMAC-SHA256 key for email lookup (emails are keyed by HMAC, never stored plaintext). ≥ 32 bytes, base64; use a different value than the email key. |
| `STATESU_RESEND` | Resend API key for verification mail. |
| `STATESU_MAIL_FROM` | `From` header for verification mail, e.g. `Statesu <verify@domain>`. The domain must be verified in Resend first. |
| `STATESU_PUBLIC_URL` | Public origin of the dashboard, e.g. `https://cms.example.com`. Used for verification links and the same-site origin check. |
| `STATESU_MEDIA_KEY` | AES-256-GCM key encrypting user-supplied R2 access keys at rest. Exactly 32 bytes, base64; use a different value than the email key. |

### Networking & storage

| Variable | Default | Description |
|---|---|---|
| `STATESU_ADDR` | `:8080` | TCP bind address; bind to `127.0.0.1:8080` behind a reverse proxy. |
| `STATESU_DB` | `db/statesu.db` | SQLite file; migrations run automatically at startup. |
| `STATESU_ALLOWED_ORIGINS` | *empty* | Extra browser origins (comma-separated) allowed for CORS/same-site writes, e.g. the Vite dev URL. |
| `STATESU_TRUSTED_PROXIES` | *empty* | CIDR ranges of proxies you trust to supply `X-Forwarded-For` for rate limiting. |
| `STATESU_R2_*` | *unset* | The server's own R2 bucket for media: `ACCOUNT_ID`, `BUCKET`, `PREFIX`, `ACCESS_KEY_ID`, `SECRET_ACCESS_KEY`, `PUBLIC_BASE_URL`. Set all six together or omit all. |

### Limits (all optional)

| Variable | Default | Description |
|---|---|---|
| `STATESU_RATE_LIMIT_RPM` | `120` | Requests/min per client IP, except auth routes. |
| `STATESU_AUTH_RATE_LIMIT_RPM` | `10` | Requests/min per client IP for register/login/verify/resend. |
| `STATESU_MAX_CONCURRENCY` | `128` | Concurrent requests; overflow gets a controlled `503`. |
| `STATESU_ARGON2_CONCURRENCY` | `2` | Parallel Argon2 hashing workers; scale to CPU cores. |
| `STATESU_MAX_POSTS_PER_USER` | `500` | Posts per user, including drafts and soft-deleted rows. |
| `STATESU_MAX_TOTAL_CONTENT_BYTES` | `32 MiB` | Stored Markdown bytes per user. |
| `STATESU_MAX_MEDIA_BYTES` | `10 MiB` | Single uploaded media file. |
| `STATESU_MAX_TOTAL_MEDIA_BYTES` | `256 MiB` | Stored media bytes per user, including variants. |
| `STATESU_MAX_MEDIA_PER_USER` | `1000` | Media records per user. |

Registration and media uploads have additional rolling-window abuse limits per IP and per user; exhaustion returns `429` with `Retry-After`. Enable Turnstile for public registration — rate limits alone cannot stop an attacker rotating IPs and verified accounts.

### Human verification & logging

| Variable | Description |
|---|---|
| `STATESU_TURNSTILE_SECRET_KEY` | Cloudflare Turnstile secret key. When set, register and login require a `turnstile` token. Pair it with the site key below. |
| `STATESU_LOG_JSON` | `true`/`1` logs one JSON object per line; otherwise plain text. |

### Dashboard build (`web/.env`)

| Variable | Description |
|---|---|
| `PUBLIC_TURNSTILE_SITE_KEY` | The widget's public site key baked into the register/login forms. Set both sides or neither. Public values are compiled in — rebuild (`make web`) after editing. |

## Building & running

```sh
make run             # builds web + binary, then cargo run --release
make api             # backend only, without rebuilding the web (fast loop)
make build           # builds web + binary, copies to bin/statesu
make web             # rebuild the embedded docs/dashboard only
make test            # cargo test --all-targets
make test-browser    # Playwright e2e for dashboard + embed
make fmt && make clippy
```

Serve the binary directly (`./bin/statesu`) in production: it opens the SQLite file, applies migrations and serves on `STATESU_ADDR`.

## Docker

The Dockerfile is multi-stage: SvelteKit build → static musl Rust build → minimal Alpine runtime running as uid `10001`, listening on `:8080`.

```sh
make docker-run       # build + run, --env-file .env, port 8080->8080, named volume /data
make docker-build
make docker-shell     # inspect the image
make docker-clean
```

`docker-compose.yml` builds the application, reads `.env`, mounts `statesu-data:/data` and listens on `:8080` inside Docker. It is proxy-independent: keep server-specific reverse proxy settings (labels, networks, published ports) in a separate Compose override outside the repository. Behind a reverse proxy, configure `STATESU_TRUSTED_PROXIES` with your proxy's IP range so rate limiting sees real client IPs.

Deployment runs on pushes to `master` via `.github/workflows/deploy.yml`, using secrets and variables from the GitHub `production` environment.

## CLI (`stscli`)

A terminal client for the HTTP API: write, edit and publish posts from your favorite editor. Full guide: [statesu.com/docs/cli](https://statesu.com/docs/cli).

```sh
cargo build --release --locked
cp target/release/stscli ~/.local/bin/   # or anywhere on PATH
```

```sh
stscli login                  # opens the website for sign-in + CLI approval;
                              # saves a write-scoped token locally
stscli blog create "Blogum"   # --slug blogum to override the slug
stscli blog use blogum        # select the blog for later commands

stscli post new "First post"    # opens $VISUAL/$EDITOR (fallback: vi)
stscli post create --file post.md --title "First post" --publish
stscli post publish first-post
stscli media upload photo.jpg   # prints a ready Markdown reference
```

Posts are drafts without `--publish`. Editing a published post changes it live immediately; there is no revision history. Slugs derive from titles — pass `--slug` when derivation fails. Every command takes `--blog <id|slug>` to act on another blog.

The default server is `https://statesu.com`; self-hosted deployments pass `--server https://your-host`. The legacy `statesu-cli` binary remains available and shares the same saved settings.

### Automation

| Variable | Effect |
|---|---|
| `STATESU_TOKEN` | Personal access token (write scope); overrides the saved token and is never written to disk. |
| `STATESU_SERVER` | Server URL; overrides the saved server. |

```sh
STATESU_SERVER=https://cms.example.com STATESU_TOKEN=su_pat_... \
  stscli post create --file post.md --title "Hello" --publish --blog blog
```

`login` signs in through the browser with an explicit CLI approval; no password or token is ever typed in the terminal. `logout` revokes the saved token. Exit codes: `0` success, `1` operation failure, `2` usage error.

## Project layout

```
src/           Rust crate: http/ (handlers+middleware), auth|blog|post/ (service+repo),
               crypto/ (hashing, encryption, tokens), cli/ (stscli), bin/ (targets)
migrations/    SQLx migrations, applied automatically on startup
web/           SvelteKit dashboard + embed script (built into assets/dist)
assets/dist/   Compiled web output, embedded via rust-embed
tests/         Rust integration tests
examples/      Example browser server
Makefile       web/build/run/api/dev/test targets and docker helpers
```

## License

MIT — see [LICENSE](LICENSE).
