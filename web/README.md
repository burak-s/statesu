# Statesu documentation

A static API guide built with SvelteKit (Svelte 5). Statesu is an API for dynamic
user statuses: update your status as it changes and retrieve it for display
through your own clients, websites, and integrations. The homepage covers registration, login, publishing, public reads,
pagination and deletion using curl, GNU wget and jq as example HTTP clients,
with switchable tabs in each request example. There are no web account forms,
sessions or state management pages. The JSON API is implemented by the Rust backend in the repository root.

## Development

From the repository root:

```sh
make dev
```

The documentation doesn't fetch API data. To try its command examples against a
local server, run `make run` separately and set `API=http://localhost:8080` in
your shell. The Rust API is bearer-only; browser sessions and cookies are not
used. Build and test with `make build`, `make test`, `make fmt`, and
`make clippy`.

## Build

`make web` runs `npm run build` and copies `web/build/` into
`assets/dist/` for embedding in the Rust binary. The homepage is prerendered
into `index.html`, so documentation and default curl examples are readable without
JavaScript. Switching terminal tabs uses client-side JavaScript. Unknown
paths, including the removed web-app pages, return 404.

## Browser tests

Install the matching Playwright Chromium once, then run the real-browser suite:

```sh
cd web
npm ci
npx playwright install chromium
npm run test:browser
# Visible browser, when a display is available:
npm run test:browser:headed
```

From the repository root, `make test-browser` runs the same suite. The harness
builds the frontend and starts the real Rust router at `http://localhost:4180`
and a consumer site at `http://127.0.0.1:4181`. It uses a fresh temporary SQLite
database and the existing recording mailer, not `.env`, a running deployment,
or real email. Mail-inspection routes exist only in the `browser_server`
example, never in the production binary. Both servers shut down after testing.
Rust, Cargo, Make and Node are required. To avoid port conflicts, set
`STATESU_BROWSER_API_PORT` and `STATESU_BROWSER_HOST_PORT` to different free ports.

Tests cover built-script delivery, cross-origin CORS and credential omission,
deep links/refresh/root paths, authenticated stored-XSS upload/update/publish,
dashboard/editor escaping, sanitizer-unavailable fallback, private/deleted
posts, ownership, integration form validation/copying, and request cancellation.
API requests are real; only mail delivery and deliberate network-failure
scenarios are substituted. No browser test is skipped or retried.

On Linux, missing browser libraries can be installed by an administrator with
`npx playwright install --with-deps chromium`. CI installs the pinned browser
revision and requires the suite to pass; its self-hosted runner must already
have those system libraries. Failures retain traces and screenshots under
`web/test-results/`, uploaded by CI. Open a trace with
`npx playwright show-trace test-results/<test>/trace.zip`.

## Search engine metadata

The homepage and public guides are prerendered with canonical URLs and social
metadata. `static/sitemap.xml` lists those pages; `static/robots.txt` also
advertises the generated demo sitemap at `/demo/blog/sitemap.xml`.
`static/llms.txt` points to public guides and source code.

`npm run build` exports `/demo/blog/` as standalone HTML using
`examples/customer-blog.js` and bundled sample API data. To export a real demo
instead, set `STATESU_DEMO_BLOG` to the slug of a published blog on `statesu.com`
(the production deploy reads it from the GitHub Actions environment variables).
The blog's public posts are fetched at build time without credentials. Rebuild
without a cached build step and redeploy to reflect changes to live posts.

After deployment, submit `https://statesu.com/sitemap.xml` and
`https://statesu.com/demo/blog/sitemap.xml` in Google Search Console.

## Layout

- `src/routes/+page.svelte` — documentation and terminal examples
- `src/lib/components/Terminal.svelte` — highlighted examples and curl/wget tabs
- `src/routes/+layout.svelte` — global styles
- `src/app.css` — plain monochrome typography and underline separators
