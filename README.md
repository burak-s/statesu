# Statesu

Statesu is a small, self-hosted **headless Markdown CMS** with a hard security focus. It ships as:

- a **backend** — a single Rust binary over HTTP: blog & post management, email-verified accounts, sessions and personal access tokens (PATs), public per-blog delivery with per-blog CORS, embedded dashboard/docs SPA;
- a **CLI** — `statesu-cli`, a terminal client to write, edit and publish posts without Git or any local post storage: the server is the source of truth.

Backend: Axum + SQLite + UUIDv7, no filesystem dependence beyond one SQLite file. Delivery and management are cleanly separated (`/public/...` vs session/PAT-guarded routes). Frontend: SvelteKit dashboard, embedded into the binary at compile time.

## Requirements

| For | Needs |
|---|---|
| Backend & CLI | Rust. A Resend account for verification mail. The toolchain is pinned in `rust-toolchain.toml` (currently `1.99.1`); with rustup installed, the right version is picked up automatically when you build. |
| Dashboard build | Node.js (Docker build uses Node 24; any recent Node works with `npm ci`). |
| Docker deployment | Docker with Docker Compose. Reverse proxy integration is optional and configured separately. |

## Quick start (development)

```sh
cp .env.example .env    # then fill in the values
make dev                # API on 127.0.0.1:8080 + Vite dashboard on :5173
```

`make dev` starts the Rust API in the background and runs the Vite dev server with HMR, proxying `/auth`, `/blogs` and `/public` to the API.

CLI development is separate and does not start the API or web server:

```sh
make dev-cli                                      # show CLI help
make dev-cli ARGS="login --server https://statesu.com"
make dev-cli ARGS="blog list"
```

To use a local server, run `make dev` in another terminal and pass `--server http://localhost:8080` to the CLI.

Register through the dashboard (`http://localhost:5173`), confirm the email, and log in. Without Turnstile configured, register/login work straight away; with Turnstile, both halves of the widget (site + secret key) must be set together.

## Configuration (server)

Copy `.env.example` to `.env`. Explicit environment variables always win over the file; the file is optional in production. Secrets are 32-byte base64 values: `openssl rand -base64 32`.

### Required

| Variable | Description |
|---|---|
| `STATESU_EMAIL_KEY` | AES-256-GCM key encrypting emails at rest. Exactly 32 bytes, base64. |
| `STATESU_EMAIL_HMAC_KEY` | HMAC-SHA256 key for email lookup (email is keyed by HMAC, never stored plaintext). ≥ 32 bytes, base64. Use a different value than the email key. |
| `STATESU_RESEND` | Resend API key for verification mail. Server-only secret. |
| `STATESU_MAIL_FROM` | `From` header for verification mail, e.g. `Statesu <verify@domain>`. The domain must be verified in Resend first. |
| `STATESU_PUBLIC_URL` | Public origin of the dashboard, e.g. `https://cms.example.com`. Bare origin: no path, no trailing slash. Used for (1) building verification links, (2) the same-site Origin check for cookie writes, (3) counting as an allowed dashboard origin. |

### Networking & storage

| Variable | Default | Description |
|---|---|---|
| `STATESU_ADDR` | `:8080` | TCP bind address. Leading `:` binds `0.0.0.0`; behind a reverse proxy bind to loopback (`127.0.0.1:8080`). Note: `localhost:8080` is rejected; use an IP. |
| `STATESU_DB` | `db/statesu.db` | SQLite file. Created on first run; migrations run automatically at startup. A legacy-schema database is detected and refused. |
| `STATESU_ALLOWED_ORIGINS` | *empty* | Extra browser origins (comma-separated) allowed as dashboard origins for CORS/same-site writes, e.g. the Vite dev URL. Not needed in the recommended single-origin deployment. |
| `STATESU_TRUSTED_PROXIES` | *empty* | CIDR ranges of proxies you own, comma-separated. Only peers inside these ranges may set `X-Forwarded-For` for rate limiting; everyone else is keyed by direct socket peer. |

### Limits (all optional)

| Variable | Default | Description |
|---|---|---|
| `STATESU_RATE_LIMIT_RPM` | `120` | Requests/min per client ip for everything except the auth routes. |
| `STATESU_AUTH_RATE_LIMIT_RPM` | `10` | Requests/min per client ip for register/login/verify/resend. |
| `STATESU_MAX_CONCURRENCY` | `128` | Concurrent requests; overflow gets a controlled `503` instead of queueing. |
| `STATESU_ARGON2_CONCURRENCY` | `2` | Parallel Argon2 password-hashing workers; scale roughly to CPU cores. |
| `STATESU_MAX_POSTS_PER_USER` | `500` | Post cap per user across all their blogs, including drafts and soft-deleted rows. |
| `STATESU_MAX_TOTAL_CONTENT_BYTES` | `33554432` (32 MiB) | Hard cap on stored Markdown bytes per user. |

### Human verification & logging

| Variable | Description |
|---|---|
| `STATESU_TURNSTILE_SECRET_KEY` | Cloudflare Turnstile *secret* key. When set, register and login require a `turnstile` token. Pair it with the widget's **site** key in the dashboard build (`web/.env`, `PUBLIC_TURNSTILE_SITE_KEY`) — set both or neither. |
| `STATESU_LOG_JSON` | `true`/`1` logs one JSON object per line (for log shippers); otherwise plain text. |

### Dashboard build configuration (`web/.env`)

| Variable | Description |
|---|---|
| `PUBLIC_TURNSTILE_SITE_KEY` | The widget's public site key baked into the register/login forms. Create the widget for the dashboard origin (same host as `STATESU_PUBLIC_URL`). Empty on either side removes the gate. |

Rebuild the dashboard (below) after editing `web/.env`: public env values are compiled in.

For Docker deployment, set `PUBLIC_TURNSTILE_SITE_KEY` as a **variable** on GitHub's `production` environment, alongside the `STATESU_TURNSTILE_SECRET_KEY` **secret** from the same Cloudflare widget. The deploy workflow passes the site key through a Compose build argument into the frontend build. Changing it requires rebuilding the image; no server-local Compose override changes are needed. For a manual build, export `PUBLIC_TURNSTILE_SITE_KEY` in the shell or put it in the root `.env` before running `docker compose up --build -d`.

## Building & running

```sh
make run      # builds web + binary, then cargo run --release
make api      # backend only, without rebuilding the docs (fast loop)
make build    # builds web + binary, copies to bin/statesu
```

- The documentation/dashboard is built from `web/` and **embedded into the binary at compile time** (`assets/dist` via rust-embed). Rebuild (`make web`) when the frontend changes.
- Serve the binary directly (`./bin/statesu`) for production; it opens the SQLite file, applies migrations and serves on `STATESU_ADDR`.
- Logs go to stderr (`STATESU_LOG_JSON=true` for JSON lines).

```sh
make web             # rebuild the embedded docs/dashboard only
make test            # cargo test --all-targets
make test-browser    # Playwright e2e for dashboard + embed
make fmt && make clippy
```

## Docker

The Dockerfile is multi-stage: SvelteKit docs build → static musl Rust build → minimal Alpine runtime running as uid `10001`, listening on `:8080`.

```sh
make docker-run       # build + run, --env-file .env, port 8080->8080, named volume /data
make docker-build
make docker-shell     # inspect the image
make docker-clean
```

`docker-compose.yml` is proxy-independent: it builds the application, reads `.env`, mounts `statesu-data:/data`, and listens on `:8080` inside Docker. It contains no Traefik labels or external networks and does not publish a host port. `STATESU_DB` and `STATESU_ADDR` are fixed by Compose so development `.env` values cannot break container networking.

### Server-specific reverse proxy configuration

Keep server-specific settings in a separate Compose override **outside the repository**. The existing Statesu server uses `/home/burak/statesu-deploy/compose.override.yml`; this file is managed on the server, not shipped with the application.

For Traefik, the override adds the application's labels and shared external Docker network to the `statesu` service. Set the domain, network, entrypoint and certificate resolver to match your existing installation, and point the service at port `8080`. This does not install or restart Traefik. Other reverse proxies can use their own overrides instead.

Deploy manually with:

```sh
docker compose -f docker-compose.yml \
  -f /home/burak/statesu-deploy/compose.override.yml \
  --project-directory . up --build -d
```

For GitHub deployment, add the **production environment variable** `STATESU_COMPOSE_OVERRIDE=/home/burak/statesu-deploy/compose.override.yml`. This is a path on the self-hosted runner, not file contents. The workflow merges it with the repository's generic Compose file. Without this variable, only the generic application stack is deployed (no external access).

When switching from the old external full Compose file, keep the same project directory and project name. Before deploying, check `docker inspect statesu --format '{{json .Mounts}}'` and `docker inspect statesu --format '{{index .Config.Labels "com.docker.compose.project"}}'`. Confirm `docker compose ... config` resolves the same data volume; if not, explicitly set the `statesu-data` volume's `name` to the existing volume name in the server-local override. Back up the database first and never use `down -v` during migration.

For Cloudflare → Traefik → Statesu, configure `STATESU_TRUSTED_PROXIES` with the actual Traefik IP/network and Cloudflare's current CIDRs ([IPv4](https://www.cloudflare.com/ips-v4), [IPv6](https://www.cloudflare.com/ips-v6)). For this server the Docker subnet is `172.20.0.0/16`; trusting it trusts every container in that subnet. The application walks `X-Forwarded-For` right-to-left across trusted hops. Traefik must separately trust Cloudflare through `forwardedHeaders.trustedIPs` in its existing configuration. Neither setting blocks direct origin access: restrict origin ports 80/443 to Cloudflare at the provider firewall or Docker-aware host firewall (published Docker ports can bypass ordinary UFW rules), and use Cloudflare **Full (strict)** TLS.

### Continuous deployment & configuration through GitHub

Deployment (`.github/workflows/deploy.yml`) runs on pushes to `master` after CI passes and takes **application environment configuration from GitHub**. The generic Compose file is versioned; optional server-specific routing comes from the runner-local override described above.

Set it up once under *Settings → Environments → `production`* (create the environment — it also gives you approval gates), then add:

| Where | Names | Why here |
|---|---|---|
| **Secrets** | `STATESU_EMAIL_KEY`, `STATESU_EMAIL_HMAC_KEY`, `STATESU_RESEND`, `STATESU_MAIL_FROM`, `STATESU_TURNSTILE_SECRET_KEY` | Not committed to the repository; written to the runner's generated `.env` and masked in logs. |
| **Variables** | `STATESU_PUBLIC_URL` (required) and optionally `STATESU_ADDR`, `STATESU_ALLOWED_ORIGINS`, `STATESU_TRUSTED_PROXIES`, the rate-limit/quotas (`STATESU_*_RPM`, `STATESU_MAX_CONCURRENCY`, `STATESU_ARGON2_CONCURRENCY`, `STATESU_MAX_POSTS_PER_USER`, `STATESU_MAX_TOTAL_CONTENT_BYTES`) and `STATESU_LOG_JSON` | Non-secret knobs; readable by anyone with repo read access. |

Set `STATESU_COMPOSE_OVERRIDE` as a production **variable** if the deployment needs a server-local Compose override. The generic Compose file fixes `STATESU_ADDR=:8080`, so a GitHub `STATESU_ADDR` value does not override the container bind address.

Before `docker compose up`, the workflow renders `.env` from these values (unset variables simply fall through to server defaults), validates that the required ones exist, and truncates any stale `.env` from previous manual runs. `STATESU_DB` stays `/data/statesu.db`, set by the compose file itself. CI does not need any of these secrets: tests run with stubbed mailer/verification and no network.

## CLI (`statesu-cli`)

A terminal client for the HTTP API. There is deliberately **no Git and no local post storage**: content lives on the server; locally only connection settings and the saved token persist (`$XDG_CONFIG_HOME/statesu/cli.json`, else `~/.config/statesu/cli.json`, else `$APPDATA/...`; written user-only, token included).

### Build & install

```sh
cargo build --release --locked
# binary: target/release/statesu-cli
cp target/release/statesu-cli ~/.local/bin/   # or anywhere on PATH
```

### Interactive usage

```sh
statesu-cli login                    # prompts for server, email, password;
                                     # mints a write-scoped PAT and saves it
statesu-cli blog list
statesu-cli blog create "Blogum"     # --slug blogum to override the slug
statesu-cli blog use blogum          # select the blog for later commands

statesu-cli post new "First post"    # opens $VISUAL/$EDITOR (fallback: vi)
statesu-cli post new "First post" --publish
statesu-cli post create --file post.md --title "First post" --publish
cat post.md | statesu-cli post create --title "First post" --slug from-stdin

statesu-cli post list [--page 1 --size 20]
statesu-cli post get first-post      # prints raw Markdown
statesu-cli post edit first-post     # updates body only; keeps status
statesu-cli post publish first-post
statesu-cli post unpublish first-post
statesu-cli post delete first-post   # confirmation prompt; --yes to skip
```

Some semantics worth knowing:

- Posts without `--publish` are saved as **drafts**.
- Editing a **published** post changes the live content immediately (warned on stderr). There is no revision/history layer.
- `post new`/`edit` need an interactive terminal. `post create` reads `--file` or piped stdin and never opens an editor.
- Slugs are derived from titles (ASCII-fold, Turkish İ/ı/ş/ğ included); the server accepts only `a-z 0-9 -` up to 63 chars. Pass `--slug` when derivation fails.
- Every command takes `--blog <id|slug>` to act on another blog without changing the selection; blog/post slug targets are resolved by scanning listings once per call.

### Automation

| Variable | Effect |
|---|---|
| `STATESU_TOKEN` | Personal access token (PAT with `content:write` for publishes). Overrides the saved token; never written to disk. |
| `STATESU_SERVER` | Server URL, overrides the saved server. |

```sh
STATESU_SERVER=https://cms.example.com STATESU_TOKEN=su_pat_... \
  statesu-cli post create --file post.md --title "Hello" --publish --blog blog --yes
```

Security-relevant behavior:

- `login` creates a temporary session, mints a write-enabled PAT, revokes the session, stores the PAT; `logout` revokes the PAT on the server (asks credentials again, since revocation is a session capability) and clears local state. If revocation fails, you are told, and you can delete the token in the dashboard.
- HTTPS everywhere, plain HTTP only for loopback hosts (`localhost`, `127.0.0.1`, `::1`, `.localhost`).
- Redirects are never followed, so a saved token never leaks through a Location hop.
- Tokens are never printed, logged or mixed into normal output; `--json` prints only JSON on stdout, warns/errors on stderr.

Exit codes: `0` success, `1` operation failure (network, API, local I/O), `2` usage/argument error.

> **Turnstile note:** when the server has a Turnstile secret configured, `login` cannot pass the challenge; mint a token in the dashboard and use `STATESU_TOKEN` instead.

## HTTP API overview

Management (`session cookie` or `Authorization: Bearer su_pat_...`; PATs are accepted on `/blogs*` only):

```
POST/GET   /auth/register  /auth/login  /auth/logout  /auth/me
POST       /auth/verify    /auth/resend
POST/GET   /auth/pats                DELETE /auth/pats/{id}
DELETE     /auth/sessions            (logout everywhere)
POST/GET   /blogs                    PATCH/DELETE /blogs/{blog_id}
PUT        /blogs/{blog_id}/origins  POST/GET /blogs/{blog_id}/posts
GET/PATCH/DELETE /blogs/{blog_id}/posts/{post_id}
```

Public delivery (unauthenticated reads, per-blog CORS allowlist):

```
GET /public/blogs/{blog_slug}
GET /public/blogs/{blog_slug}/posts            (?page=&size=)
GET /public/blogs/{blog_slug}/posts/{post_slug}
```

Errors are always `{"error": "<stable message>"}` with the status assigned by the domain. Rate-limited responses carry `Retry-After`.

## Project layout

```
src/           Rust crate: http/ (handlers+middleware), auth|blog|post/ (service+repo),
               crypto/ (hashing, encryption, tokens), cli/ (statesu-cli), bin/ (targets)
migrations/    SQLx migrations, applied automatically on startup
web/           SvelteKit dashboard + embed script (built into assets/dist)
assets/dist/   Compiled web output, embedded via rust-embed
tests/         Rust integration tests (auth, API contracts, HTTP behavior)
examples/      Example browser server
Makefile       web/build/run/api/dev/test targets and docker helpers
```

## License

MIT — see [LICENSE](LICENSE).
