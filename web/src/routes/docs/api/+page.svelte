<script>
	import Seo from '#lib/components/Seo.svelte';
	import Terminal from '#lib/components/Terminal.svelte';

	const endpoints = [
		['GET', '/blogs', 'List your blogs.'],
		['POST', '/blogs', 'Create a blog: name, slug, optional domain.'],
		['PATCH', '/blogs/{blogID}', 'Rename a blog or update its domain.'],
		['PUT', '/blogs/{blogID}/origins', 'Replace allowed browser origins.'],
		['PUT', '/blogs/{blogID}/appearance', 'Replace the saved embed appearance; null clears it.'],
		['GET', '/blogs/{blogID}/posts', 'List your posts, including drafts.'],
		['POST', '/blogs/{blogID}/posts', 'Create a post: slug, title, content_markdown, optional status.'],
		['GET', '/blogs/{blogID}/posts/{postID}', 'Read a post.'],
		['PATCH', '/blogs/{blogID}/posts/{postID}', 'Edit a post or change draft/published status.'],
		['DELETE', '/blogs/{blogID}/posts/{postID}', 'Soft-delete a post; its slug stays reserved. Returns 204.'],
		['POST', '/blogs/{blogID}/media', 'Upload one image (raw bytes; see Media below).'],
		['GET', '/blogs/{blogID}/media', 'List uploaded media.'],
		['DELETE', '/blogs/{blogID}/media/{mediaID}', 'Delete media and its objects. Returns 204.'],
		['GET/POST', '/blogs/{blogID}/storage-connections', 'Manage your own R2 buckets.'],
		['PUT', '/blogs/{blogID}/media-storage', 'Choose where new media uploads go.']
	];
	const publicEndpoints = [
		['/public/blogs/{blogSlug}', 'Blog profile, including the saved embed appearance when set.'],
		['/public/blogs/{blogSlug}/posts', 'Published posts, newest first.'],
		['/public/blogs/{blogSlug}/posts/{postSlug}', 'One published post with Markdown.']
	];
	const setup = `API=https://statesu.com
read -r -s -p 'API token: ' TOKEN && echo`;
	const create = `curl --fail-with-body -sS "$API/blogs" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' \\
  --data-binary '{"name":"Writing desk","slug":"writing-desk"}'`;
	const publish = `# Replace BLOG_ID with the id from the blog response.
curl --fail-with-body -sS "$API/blogs/BLOG_ID/posts" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' \\
  --data-binary '{"slug":"hello-world","title":"Hello world","content_markdown":"# Hello","status":"published"}'`;
	const uploadMedia = `# Send the image bytes as the body; the response carries public_url.
curl --fail-with-body -sS "$API/blogs/BLOG_ID/media" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: image/webp' \
  --data-binary @photo.webp`;
</script>

<Seo
	title="Markdown Blog API Reference"
	description="Create blogs and publish Markdown over HTTP. Use the Statesu API reference to authenticate, manage posts, and fetch published content as public JSON."
	path="/docs/api"
	type="article"
/>

<section class="intro">
	<p class="eyebrow">API guide</p>
	<h1>Markdown blog API reference</h1>
	<p class="lead">Manage content over HTTP, or read published posts as public JSON. For ready-made terminal commands, use the <a href="/docs/cli">CLI</a>.</p>
</section>

<section>
	<h2>Authentication</h2>
	<p>Base URL: <code>https://statesu.com</code>. Register and verify your email in the <a href="/dashboard">dashboard</a>, then create a personal access token in <strong>API tokens</strong>. Use a write-capable token for mutations. The secret is shown once and can be revoked at any time.</p>
	<p>Send <code>Authorization: Bearer $TOKEN</code> for management requests and <code>Content-Type: application/json</code> for JSON bodies. Keep tokens in server-side environments or secret managers, never public website code.</p>
	<Terminal code={setup} />
	<p>The dashboard's <code>/auth</code> endpoints use sessions and CSRF protection, not personal access tokens. Tokens authenticate blog and post management requests only.</p>
</section>

<section>
	<h2>Create a blog and publish</h2>
	<p>Save the returned blog ID for post requests.</p>
	<Terminal code={create} />
	<Terminal code={publish} />
	<p>Omit <code>status</code> for a draft. Slugs use lowercase letters, digits and hyphens (up to 63 bytes). Titles are single-line text up to 256 bytes; Markdown is limited to 256 KiB. Editing published content changes it live.</p>
</section>

<section>
	<h2>Management endpoints</h2>
	<p>All routes below require authentication and ownership of the blog. Unknown and foreign resources return 404. Listing posts supports <code>?page=1&amp;size=20</code>, with size capped at 100.</p>
	<div class="table-scroll">
		<table>
			<thead><tr><th>Method / endpoint</th><th>Purpose</th></tr></thead>
			<tbody>
				{#each endpoints as [method, path, purpose]}
					<tr><td><code>{method} {path}</code></td><td>{purpose}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<p>Set origins with <code>{'{"origins":["https://example.com"]}'}</code>. The request replaces the entire list. See <a href="/docs/integration">web integration</a> for CORS and hosting setup.</p>
	<p>Read saved styling from <code>GET /blogs</code>. Write it with <code>PUT /blogs/&#123;blogID&#125;/appearance</code> using a write-capable bearer token or authenticated dashboard session. The CLI uses this same API.</p>
	<Terminal code={'{\n  "embed_appearance": {\n    "version": 1,\n    "markdown": {\n      "code_background": "#f3f4f6",\n      "code_padding": 16,\n      "quote_border_width": 3,\n      "table_cell_padding": 8\n    }\n  }\n}'} language="json" />
	<p>Set the embed appearance with <code>{'{"embed_appearance":{"version":1,…}}'}</code> — the full document replaces whatever was saved, and an explicit <code>{'{"embed_appearance":null}'}</code> clears it. Values are validated tokens, never CSS text; the shape and bounds are documented in the <a href="/docs/integration">web integration guide</a>. Blog responses include the saved appearance as <code>embed_appearance</code> when one is set.</p>
</section>

<section>
	<h2>Media</h2>
	<p>Upload JPEG, PNG or WebP images as the raw request body; the stored file is re-encoded and published on the blog's storage bucket. The response includes the permanent public URL to paste into Markdown, plus responsive variant metadata for renderers that support it.</p>
	<Terminal code={uploadMedia} />
	<p>Each file can be up to 10 MiB, with a 256 MiB total budget per user. Aspect-ratio variants at 640/1280/1920 px are generated server-side; failed uploads are cleaned up. Deleting media removes its objects; posts that still reference the URL lose the image, so confirm before deleting. Animated WebP and oversized images are rejected.</p>
	<h3>Your own R2 bucket</h3>
	<p>Blog-scoped storage connections point Statesu at a Cloudflare R2 bucket you own: an account ID, bucket name, statesu-dedicated key prefix, public HTTPS media base URL and an R2 API token scoped to that single bucket. Secrets are stored encrypted and never returned. Media already uploaded stays on its original bucket; switching the preference only affects new uploads.</p>
</section>

<section>
	<h2>Public delivery</h2>
	<p>These GET endpoints need no token and return published content only. Draft, withdrawn and deleted posts are not delivered. Public post lists support the same page and size parameters.</p>
	<div class="table-scroll">
		<table>
			<thead><tr><th>GET endpoint</th><th>Purpose</th></tr></thead>
			<tbody>
				{#each publicEndpoints as [path, purpose]}
					<tr><td><code>{path}</code></td><td>{purpose}</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	<Terminal code={'curl --fail-with-body -sS \\\n  https://statesu.com/public/blogs/writing-desk/posts/hello-world'} />
	<p>Browser reads require an allowed origin; server-side requests do not. Treat returned Markdown as untrusted and sanitize it when rendering. When a post references uploaded media, the response gains a <code>media</code> array with the canonical source URL, source dimensions and responsive variants — extra data, the Markdown itself is unchanged. The blog profile response likewise gains <code>embed_appearance</code> while one is saved; it is the same document the v2 embed applies.</p>
</section>

<section>
	<h2>Errors and limits</h2>
	<Terminal code={'{"error":"slug already in use"}'} language="json" />
	<ul>
		<li><strong>400 / 413 / 415:</strong> invalid input, oversized body or unsupported content type.</li>
		<li><strong>401 / 403:</strong> invalid authentication, insufficient permissions or an account quota reached.</li>
		<li><strong>404 / 409:</strong> unknown resource or a conflicting slug/domain.</li>
		<li><strong>429:</strong> rate limit reached; respect the <code>Retry-After</code> header. Uploads allow 20 attempts/minute and 200/day per IP, plus 10/minute and 100/day per user; failed uploads count toward these limits.</li>
		<li><strong>500:</strong> server error.</li>
	</ul>
	<p>Deleting a post removes it from public delivery but keeps its slug reserved. Revoking a token invalidates subsequent requests immediately.</p>
</section>
