# Statesu documentation

A static API guide built with SvelteKit (Svelte 5). Statesu is an API for dynamic
user statuses: update your status as it changes and retrieve it for display
through your own clients, websites, and integrations. The homepage covers registration, login, publishing, public reads,
pagination and deletion using curl, GNU wget and jq as example HTTP clients,
with switchable tabs in each request example. There are no web account forms,
sessions or state management pages. The JSON API remains in the Go backend.

## Development

From the repository root:

```sh
make dev
```

The documentation doesn't fetch API data. To try its command examples against a
local server, run `make run` separately and set `API=http://localhost:8080` in
your shell.

## Build

`make web` runs `npm run build` and copies `web/build/` into
`internal/spa/dist/` for embedding in the Go binary. The homepage is prerendered
into `index.html`, so documentation and default curl examples are readable without
JavaScript. Switching terminal tabs uses client-side JavaScript. Unknown
paths, including the removed web-app pages, return 404.

## Search engine metadata

The prerendered homepage includes a single title and description, a canonical URL,
Open Graph and Twitter metadata, and JSON-LD describing the API documentation.
`static/robots.txt` allows documentation crawling and excludes API routes;
`static/sitemap.xml` lists the homepage, the site's only documentation page.
Crawler exclusions are not access controls: states remain public.

The production URL is `https://statesu.com/`. If the domain changes, update the
canonical URL in `src/routes/+page.svelte` and the URLs in both crawler files.
Add new documentation pages to the sitemap when they are introduced. After
deployment, submit `https://statesu.com/sitemap.xml` in Google Search Console.

## Layout

- `src/routes/+page.svelte` — documentation and terminal examples
- `src/lib/components/Terminal.svelte` — highlighted examples and curl/wget tabs
- `src/routes/+layout.svelte` — global styles
- `src/app.css` — plain monochrome typography and underline separators
