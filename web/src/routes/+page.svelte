<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const title = 'Statesu — a small headless Markdown CMS';
	const description = 'An HTTP API for Markdown blogs. Create blogs and posts, publish drafts, and read published posts from your own website over a per-blog CORS policy.';
	const canonicalUrl = 'https://statesu.com/';
	const structuredData = {
		'@context': 'https://schema.org',
		'@type': 'TechArticle',
		'@id': `${canonicalUrl}#documentation`,
		headline: title,
		description,
		url: canonicalUrl,
		inLanguage: 'en',
		about: {
			'@type': 'WebAPI',
			name: 'Statesu',
			url: canonicalUrl,
			description: 'A headless Markdown CMS: authenticated management for blogs, posts, domains and origins, with public JSON reads for your own website.'
		}
	};

	const examples = {
		setup: `API=https://statesu.com
# For a local server: API=http://localhost:8080
EMAIL=you@example.com`,
		register: `# Bash: prompt without saving your password in shell history.
read -r -s -p 'Password: ' PASSWORD; printf '\\n'
TOKEN=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \\
  '{email: $email, password: $password}' | \\
  curl --fail-with-body -sS "$API/auth/register" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.token')
unset PASSWORD`,
		login: `read -r -s -p 'Password: ' PASSWORD; printf '\\n'
TOKEN=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \\
  '{email: $email, password: $password}' | \\
  curl --fail-with-body -sS "$API/auth/login" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.token')
unset PASSWORD`,
		createBlog: `BLOG_ID=$(jq -n '{name: "Writing desk", slug: "writing-desk"}' | \\
  curl --fail-with-body -sS "$API/blogs" \\
    -H "Authorization: Bearer $TOKEN" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.id')`,
		createPost: `jq -n '{slug: "hello-world", title: "Hello", \\
  content_markdown: "# Hello\\n\\nFirst post.", \\
  status: "published"}' | \\
  curl --fail-with-body -sS "$API/blogs/$BLOG_ID/posts" \\
    -H "Authorization: Bearer $TOKEN" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.id'`,
		origins: `# One per line as a JSON array; replace the whole list.
curl --fail-with-body -sS -X PUT "$API/blogs/$BLOG_ID/origins" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' \\
  --data-binary '{"origins":["https://example.com","http://localhost:5173"]}' | jq .`,
		readPublic: `curl --fail-with-body -sS "$API/public/blogs/writing-desk/posts" | jq .

# Only the Markdown body of one post — useful in templating.
curl --fail-with-body -sS "$API/public/blogs/writing-desk/posts/hello-world" \\
  | jq -r '.content_markdown'`,
		editPost: `PATCH_BODY=$(jq -n '{content_markdown: "# Hello\\n\\nUpdated text."}')
curl --fail-with-body -sS -X PATCH "$API/blogs/$BLOG_ID/posts/$POST_ID" \\
  -H "Authorization: Bearer $TOKEN" \\
  -H 'Content-Type: application/json' --data-binary "$PATCH_BODY" | jq .`,
		remove: `curl --fail-with-body -sS -X DELETE "$API/blogs/$BLOG_ID/posts/$POST_ID" \\
  -H "Authorization: Bearer $TOKEN"
# Success: HTTP 204, no response body.`,
		response: `{
  "slug": "hello-world",
  "title": "Hello",
  "content_markdown": "# Hello\\n\\nFirst post.",
  "published_at": 1760000000
}`,
		display: `const response = await fetch(
  'https://statesu.com/public/blogs/writing-desk/posts/hello-world'
);
if (!response.ok) throw new Error('Could not fetch post');
const post = await response.json();
document.querySelector('#title').textContent = post.title;
// Markdown is rendered by your own renderer — keep raw HTML disabled.`
	};
	const wgetTokenHint = `# Bash: prompt without saving your secret in shell history.
read -r -s -p 'Token: ' TOKEN; printf '\\n'`;
	const wgetExamples = {
		register: `read -r -s -p 'Password: ' PASSWORD; printf '\\n'
BODY=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \
  '{email: $email, password: $password}')
wget -O- --header='Content-Type: application/json' \
  --post-data="$BODY" "$API/auth/register" | jq .
# 202 accepted; open the verification link in the mail, then sign in at
# /dashboard and mint an su_pat_ token there.
unset PASSWORD BODY`,
		login: wgetTokenHint,
		createBlog: `BODY=$(jq -n '{name: "Writing desk", slug: "writing-desk"}')
BLOG_ID=$(wget -O- --header="Authorization: Bearer $TOKEN" \\
  --header='Content-Type: application/json' \\
  --post-data="$BODY" "$API/blogs" | jq -er '.id')`,
		createPost: `BODY=$(jq -n '{slug: "hello-world", title: "Hello", \\
  content_markdown: "# Hello", status: "published"}')
POST_ID=$(wget -O- --header="Authorization: Bearer $TOKEN" \\
  --header='Content-Type: application/json' \\
  --post-data="$BODY" "$API/blogs/$BLOG_ID/posts" | jq -er '.id')`,
		origins: `BODY='{"origins":["https://example.com"]}'
wget -O- --method=PUT "$API/blogs/$BLOG_ID/origins" \\
  --header="Authorization: Bearer $TOKEN" \\
  --header='Content-Type: application/json' \\
  --post-data="$BODY" | jq .`,
		readPublic: `wget -O- "$API/public/blogs/writing-desk/posts" | jq .

# One post only — its Markdown body.
wget -O- "$API/public/blogs/writing-desk/posts/hello-world" \
  | jq -r '.content_markdown'`,
		remove: `wget -O- --method=DELETE "$API/blogs/$BLOG_ID/posts/$POST_ID" \\
  --header="Authorization: Bearer $TOKEN"
# Success: HTTP 204, no response body.`
	};
	const endpoints = [
		['POST', '/auth/register', 'No', 'Create an unverified account; sends a verification mail. 202.'],
		['POST', '/auth/verify', 'No', 'Confirm the email with the link token; single use, 30-minute expiry. 200.'],
		['POST', '/auth/resend', 'No', 'Re-send the verification mail; throttled, generic answer. 202.'],
		['POST', '/auth/login', 'No', 'Dashboard sign-in; sets an HttpOnly session cookie. No token body.'],
		['POST', '/auth/logout', 'Session', 'Revoke this session and clear its cookies. 204.'],
		['DELETE', '/auth/sessions', 'Session', 'Sign out everywhere: revoke every session. 204.'],
		['GET', '/auth/me', 'Session', 'Check the session; returns id and email. 200.'],
		['POST', '/auth/pats', 'Session', 'Mint a personal access token; the su_pat_ secret is shown once. 201.'],
		['GET', '/auth/pats', 'Session', 'List your tokens (metadata only; never the secret). 200.'],
		['DELETE', '/auth/pats/{patID}', 'Session', 'Revoke a token; clients stop working immediately. 204.'],
		['POST', '/blogs', 'Token', 'Create a blog: name, slug, optional domain. 201.'],
		['GET', '/blogs', 'Token', 'List your blogs, newest first. 200.'],
		['PATCH', '/blogs/{blogID}', 'Token', 'Rename or change/clear the domain. 200.'],
		['PUT', '/blogs/{blogID}/origins', 'Token', 'Replace the exact browser origins allowed to call delivery. 200.'],
		['POST', '/blogs/{blogID}/posts', 'Token', 'Create a post: slug, title, content_markdown, optional status. 201.'],
		['GET', '/blogs/{blogID}/posts', 'Token', 'List your posts, newest first; page and size. 200.'],
		['GET', '/blogs/{blogID}/posts/{postID}', 'Token', 'Read one of your posts. 200.'],
		['PATCH', '/blogs/{blogID}/posts/{postID}', 'Token', 'Edit fields or toggle draft/published. 200.'],
		['DELETE', '/blogs/{blogID}/posts/{postID}', 'Token', 'Soft-delete a post; its slug stays reserved. 204.'],
		['GET', '/public/blogs/{blogSlug}', 'No', 'Public blog profile: name, slug, domain. 200.'],
		['GET', '/public/blogs/{blogSlug}/posts', 'No', 'Published posts only, newest first; page and size. 200.'],
		['GET', '/public/blogs/{blogSlug}/posts/{postSlug}', 'No', 'One published post with its Markdown. 200.']
	];
</script>

<svelte:head>
	<title>{title}</title>
	<meta name="description" content={description} />
	<link rel="canonical" href={canonicalUrl} />
	<meta property="og:type" content="website" />
	<meta property="og:site_name" content="Statesu" />
	<meta property="og:title" content={title} />
	<meta property="og:description" content={description} />
	<meta property="og:url" content={canonicalUrl} />
	<meta property="og:locale" content="en_US" />
	<meta name="twitter:card" content="summary" />
	<meta name="twitter:title" content={title} />
	<meta name="twitter:description" content={description} />
	{@html `<script type="application/ld+json">${JSON.stringify(structuredData)}</script>`}
</svelte:head>

<header class="header">
	<a class="brand" href="/">Statesu</a>
	<nav aria-label="Documentation">
		<a href="#quickstart">Quick start</a>
		<a href="#delivery">Delivery & CORS</a>
		<a href="#reference">API reference</a>
		<a href="/dashboard">Dashboard</a>
		<a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a>
	</nav>
</header>

<main>
	<section class="intro">
		<p class="eyebrow">A small headless Markdown CMS.</p>
		<h1>Write in Markdown.<br />Publish to your site.</h1>
		<p class="lead">Blogs, drafts and published posts behind one API. Your website stays yours: read published posts as JSON and render them however you like.</p>
		<p>Manage posts from the <a href="/dashboard">dashboard</a> or the API below. Posts can be drafts while you work, then published with one call. Drafts, withdrawn and deleted posts never appear through delivery routes.</p>
	</section>

	<section id="quickstart">
		<h2>Get started with the API</h2>
		<p>The examples use curl or wget, but the same endpoints work with any HTTP client. You need Bash, <a href="https://jqlang.org/">jq</a>, and either curl 7.76+ or GNU wget 1.15+; switch clients in each example. Run all commands in the same shell so your variables stay available.</p>
		<Terminal code={examples.setup} />

		<h3>01 / Create an account</h3>
		<p>Register once: 8–72 byte passwords with at least one letter and one digit. The address is confirmed by a verification mail before the account works; links expire after 30 minutes and can be re-sent from the dashboard. For scripts and CLI tools, mint a personal access token (<code>su_pat_…</code>) in the dashboard's <a href="/dashboard">API tokens</a> panel and pass it as <code>Authorization: Bearer $TOKEN</code>. Tokens are shown exactly once at creation, expire on your chosen date or never expire, and can be revoked at any time.</p>
		<Terminal variants={{ curl: examples.register, wget: wgetExamples.register }} />

		<h3>02 / Create a blog</h3>
		<p>A blog holds posts and its own delivery rules. The slug becomes the public URL path; the domain is optional metadata. A user owns up to 20 blogs.</p>
		<Terminal variants={{ curl: examples.createBlog, wget: wgetExamples.createBlog }} />

		<h3>03 / Write a post</h3>
		<p>Markdown is stored as the canonical content: a leading byte-order mark is stripped, line endings are normalized to LF, and control characters (NUL, other C0/C1, DEL) are rejected. Slug rules: lowercase letters, digits and hyphens, at most 63 bytes; titles are single-line printable text up to 256 bytes; Markdown up to 256 KiB. Omit <code>status</code> to create a draft, or set it to <code>published</code> to publish immediately.</p>
		<Terminal variants={{ curl: examples.createPost, wget: wgetExamples.createPost }} />

		<h3>04 / Choose who can read it</h3>
		<p>Delivery routes are public JSON reads of published posts. Drafts stay private. Browser access is scoped per blog: record the exact origins of your website, and only those origins receive the CORS header. No suffix matching, no reflected origins, no credentials.</p>
		<Terminal variants={{ curl: examples.origins, wget: wgetExamples.origins }} />

		<h3>05 / Read it from your website</h3>
		<p>Reads return JSON with your Markdown. Listing and reading are always newest-first per blog, page or size capped at 100. Unknown blogs and posts return 404.</p>
		<Terminal variants={{ curl: examples.readPublic, wget: wgetExamples.readPublic }} />
		<p>A published post looks like this; management-only fields (<code>status</code>, owner IDs) never appear here.</p>
		<Terminal code={examples.response} language="json" />

		<h3 id="safe-rendering">Security / Rendering Markdown safely</h3>
		<p><strong>Render as plain or sanitized text:</strong> <code>content_markdown</code> is untrusted user input, delivered verbatim. Render it with a Markdown renderer you trust and keep raw HTML disabled or the output sanitized. In browsers, assign titles and slugs to <code>textContent</code>; never pass API fields to <code>innerHTML</code> unescaped.</p>
		<Terminal code={examples.display} language="javascript" />
	</section>

	<section id="delivery">
		<h2>Delivery and CORS, precisely scoped</h2>
		<h3>Which origins are allowed</h3>
		<p><code>PUT /blogs/{'{blogID}'}/origins</code> replaces the whole list. Origins are normalized to <code>scheme://host[:port]</code> and compared exactly — one origin per line, no paths, queries or credentials. Hosts are strict RFC 1123 names (ASCII labels, punycode <code>xn--</code> for international domains, IPv4-like names, or bracketed IPv6 literals like <code>[::1]:5173</code>); ports must be 1–65535. Production and localhost preview coexist in the same list; up to 32 origins per blog.</p>
		<h3>What CORS does and does not do</h3>
		<p>CORS governs the browser only. <code>curl</code>, scrapers and server-to-server calls can read public posts regardless of the origin list — that is the point of published content; keep secrets out of posts, and never put an API token in code that runs on your website.</p>
		<h3>Drafts, withdrawal and deletion</h3>
		<p>Publishing sets <code>published_at</code>; withdrawing clears it and the post stops being delivered. Deleting soft-deletes the post immediately in every listing, while reserving its slug so old URLs never silently point at new content.</p>
	</section>

	<section id="reference">
		<h2>API reference</h2>
		<p>Base URL: <code>https://statesu.com</code>. Bodies are JSON; send <code>Content-Type: application/json</code> for JSON bodies and <code>Authorization: Bearer $TOKEN</code> for token-authenticated requests. The dashboard itself uses an HttpOnly session cookie plus a CSRF header — never a token — and <code>/auth</code> endpoints refuse tokens by design. <code>GET /blogs</code> and blog settings require the blog to belong to you; unknown and foreign resources both return 404.</p>
		<p>Requests are rate-limited per client IP. Exceeding a limit returns 429 with a <code>Retry-After</code> header; public delivery reads have a separate, larger limit.</p>
		<div class="table-scroll">
			<table>
				<thead><tr><th>Method / endpoint</th><th>Auth</th><th>Behavior</th></tr></thead>
				<tbody>
					{#each endpoints as [method, path, auth, description]}
						<tr><td><code>{method} {path}</code></td><td>{auth}</td><td>{description}</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
		<h3>Edit and manage posts</h3>
		<p>Use the post ID saved when creating, or <code>id</code> from a listing. Editing a published post changes it live; there is no revision history yet.</p>
		<Terminal variants={{ curl: examples.editPost, wget: wgetExamples.remove }} />
		<h3>Delete a post</h3>
		<Terminal variants={{ curl: examples.remove, wget: wgetExamples.remove }} />
		<h3>Error responses and HTTP status codes</h3>
		<Terminal code={'{"error":"slug already in use"}'} language="json" />
		<ul>
			<li><strong>400</strong> — invalid JSON, unknown fields, malformed emails/passwords, invalid slugs, titles, content or pagination.</li>
			<li><strong>401</strong> — incorrect credentials, a missing, invalid or expired token, or an unverified address on login (403 with <code>email not verified</code>).</li>
			<li><strong>403</strong> — a limit is reached (such as the per-user blog quota), a read-only token attempted a write, or a cross-site browser write was refused.</li>
			<li><strong>404</strong> — unknown or foreign blog or post or token; drafts are also invisible through public routes.</li>
			<li><strong>409</strong> — a duplicate slug or domain, an already-registered email, or a <code>(blog, slug)</code> post conflict.</li>
			<li><strong>413</strong> — the body is too large: 256 KiB for blog and post bodies, 16 KiB for auth.</li>
			<li><strong>415</strong> — the body is not <code>application/json</code>.</li>
			<li><strong>429</strong> — rate limit exceeded; honor <code>Retry-After</code>.</li>
			<li><strong>500</strong> — server error; retry later.</li>
		</ul>
		<p><code>curl --fail-with-body</code> preserves the error body and exits nonzero on HTTP errors. Logout revokes the session server-side; a token is revoked the moment you delete it in the dashboard (401 everywhere afterwards). Links from verification mails point at your deployment's own origin — configured, never inferred from request headers.</p>
	</section>
</main>

<footer>
	<span>Statesu — a small headless Markdown CMS. <a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a></span>
	<a href="/dashboard">Dashboard</a>
	<a href="#quickstart">Back to quick start ↑</a>
</footer>
