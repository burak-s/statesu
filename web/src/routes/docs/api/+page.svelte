<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const endpoints = [
		['GET', '/blogs', 'List your blogs.'],
		['POST', '/blogs', 'Create a blog: name, slug, optional domain.'],
		['PATCH', '/blogs/{blogID}', 'Rename a blog or update its domain.'],
		['PUT', '/blogs/{blogID}/origins', 'Replace allowed browser origins.'],
		['GET', '/blogs/{blogID}/posts', 'List your posts, including drafts.'],
		['POST', '/blogs/{blogID}/posts', 'Create a post: slug, title, content_markdown, optional status.'],
		['GET', '/blogs/{blogID}/posts/{postID}', 'Read a post.'],
		['PATCH', '/blogs/{blogID}/posts/{postID}', 'Edit a post or change draft/published status.'],
		['DELETE', '/blogs/{blogID}/posts/{postID}', 'Soft-delete a post; its slug stays reserved. Returns 204.']
	];
	const publicEndpoints = [
		['/public/blogs/{blogSlug}', 'Blog profile.'],
		['/public/blogs/{blogSlug}/posts', 'Published posts, newest first.'],
		['/public/blogs/{blogSlug}/posts/{postSlug}', 'One published post with Markdown.']
	];
	const setup = `API=https://statesu.com
# Bash: use a dashboard token, not your account password.
read -r -s -p 'API token: ' TOKEN; printf '\\n'`;
	const create = `curl --fail-with-body -sS "$API/blogs" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' \\
  --data-binary '{"name":"Writing desk","slug":"writing-desk"}'`;
	const publish = `# Replace BLOG_ID with the id from the blog response.
curl --fail-with-body -sS "$API/blogs/BLOG_ID/posts" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' \\
  --data-binary '{"slug":"hello-world","title":"Hello world","content_markdown":"# Hello","status":"published"}'`;
</script>

<svelte:head>
	<title>API guide — Statesu</title>
	<meta name="description" content="Authenticate with Statesu tokens, manage blogs and posts over HTTP, and read published content as public JSON." />
	<link rel="canonical" href="https://statesu.com/docs/api" />
</svelte:head>

<section class="intro">
	<p class="eyebrow">API guide</p>
	<h1>Build your own workflow.</h1>
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
	<p>These Bash examples use curl 7.76 or newer. Save the returned blog ID for post requests.</p>
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
	<p>Browser reads require an allowed origin; server-side requests do not. Treat returned Markdown as untrusted and sanitize it when rendering.</p>
</section>

<section>
	<h2>Errors and limits</h2>
	<Terminal code={'{"error":"slug already in use"}'} language="json" />
	<ul>
		<li><strong>400 / 413 / 415:</strong> invalid input, oversized body or unsupported content type.</li>
		<li><strong>401 / 403:</strong> invalid authentication, insufficient permissions or an account quota reached.</li>
		<li><strong>404 / 409:</strong> unknown resource or a conflicting slug/domain.</li>
		<li><strong>429:</strong> rate limit reached; respect the <code>Retry-After</code> header.</li>
		<li><strong>500:</strong> server error.</li>
	</ul>
	<p>Deleting a post removes it from public delivery but keeps its slug reserved. Revoking a token invalidates subsequent requests immediately.</p>
</section>
