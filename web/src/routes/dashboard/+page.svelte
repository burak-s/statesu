<script>
	import {
		isEmail,
		loadToken,
		request,
		storeToken
	} from '#lib/api.js';

	// Session ----------------------------------------------------------------
	let token = $state(null);
	let me = $state(null);
	let authMode = $state('login');
	let authEmail = $state('');
	let authPassword = $state('');
	let authError = $state('');
	let authBusy = $state(false);

	// Blogs ------------------------------------------------------------------
	let blogs = $state([]);
	let selectedBlogId = $state(null);
	let newBlogName = $state('');
	let newBlogSlug = $state('');
	let newBlogError = $state('');

	// Blog settings ----------------------------------------------------------
	let domainDraft = $state('');
	let originsDraft = $state('');
	let settingsError = $state('');
	let settingsSaved = $state(false);

	// Posts ------------------------------------------------------------------
	let posts = $state([]);
	let postsError = $state('');
	let editor = $state(null); // null | {id?,blogId,slug,title,content,status,published_at}
	let editorError = $state('');
	let busy = $state(false);

	const selectedBlog = $derived(blogs.find((b) => b.id === selectedBlogId) ?? null);

	$effect(() => {
		const saved = loadToken();
		if (saved) initialize(saved);
	});

	async function initialize(savedToken) {
		const check = await request('/auth/me', { token: savedToken });
		if (!check.ok) {
			storeToken(null);
			return;
		}
		token = savedToken;
		me = check.data;
		await refreshBlogs();
	}

	async function submitAuth(event) {
		event.preventDefault();
		authError = '';
		if (!isEmail(authEmail) || authPassword.length < 8) {
			authError = 'Enter a valid email and a password of at least 8 characters.';
			return;
		}
		authBusy = true;
		const result = await request(`/auth/${authMode}`, {
			method: 'POST',
			body: { email: authEmail.trim(), password: authPassword }
		});
		authBusy = false;
		if (!result.ok) {
			authError = result.error;
			return;
		}
		token = result.data.token;
		storeToken(token);
		authPassword = '';
		await refreshBlogs();
	}

	async function refreshBlogs() {
		const meResult = await request('/auth/me', { token });
		if (meResult.ok) me = meResult.data;
		await listBlogs();
	}

	async function listBlogs() {
		const result = await request('/blogs', { token });
		if (!result.ok) return;
		blogs = result.data;
		await selectBlog(selectedBlogId ?? blogs[0]?.id ?? null);
	}

	async function selectBlog(id) {
		selectedBlogId = id;
		posts = [];
		postsError = '';
		editor = null;
		editorError = '';
		settingsError = '';
		settingsSaved = false;
		if (!id) return;
		domainDraft = selectedBlog?.domain ?? '';
		originsDraft = (selectedBlog?.origins ?? []).join('\n');
		const result = await request(`/blogs/${id}/posts`, { token });
		if (!result.ok) {
			postsError = result.error;
			return;
		}
		posts = result.data.items;
	}

	async function createBlog(event) {
		event.preventDefault();
		newBlogError = '';
		const result = await request('/blogs', {
			token,
			method: 'POST',
			body: { name: newBlogName.trim(), slug: newBlogSlug.trim() }
		});
		if (!result.ok) {
			newBlogError = result.error;
			return;
		}
		newBlogName = '';
		newBlogSlug = '';
		await listBlogs();
	}

	async function saveSettings() {
		settingsError = '';
		settingsSaved = false;
		const domain = domainDraft.trim();
		const origins = originsDraft
			.split('\n')
			.map((line) => line.trim())
			.filter(Boolean);

		const originsResult = await request(`/blogs/${selectedBlogId}/origins`, {
			token,
			method: 'PUT',
			body: { origins }
		});
		if (!originsResult.ok) {
			settingsError = originsResult.error;
			return;
		}
		if (domain !== (selectedBlog?.domain ?? '')) {
			const patch = await request(`/blogs/${selectedBlogId}`, {
				token,
				method: 'PATCH',
				body: { domain: domain === '' ? null : domain }
			});
			if (!patch.ok) {
				settingsError = patch.error;
				return;
			}
		}
		settingsSaved = true;
		await listBlogs();
	}

	function startDraftPost() {
		editorError = '';
		editor = { id: null, slug: '', title: '', content_markdown: '', status: 'draft' };
	}

	function startEditPost(post) {
		editorError = '';
		editor = {
			id: post.id,
			slug: post.slug,
			title: post.title,
			content_markdown: post.content_markdown,
			status: post.status,
			published_at: post.published_at
		};
	}

	async function saveEditor() {
		editorError = '';
		busy = true;
		const path = editor.id
			? `/blogs/${selectedBlogId}/posts/${editor.id}`
			: `/blogs/${selectedBlogId}/posts`;
		const body = editor.id
			? { slug: editor.slug.trim(), title: editor.title, content_markdown: editor.content_markdown }
			: {
					slug: editor.slug.trim(),
					title: editor.title,
					content_markdown: editor.content_markdown,
					status: editor.status
				};
		const result = await request(path, { token, method: editor.id ? 'PATCH' : 'POST', body });
		busy = false;
		if (!result.ok) {
			editorError = result.error;
			return;
		}
		editor = null;
		await selectBlog(selectedBlogId);
	}

	async function togglePublish(post) {
		busy = true;
		const result = await request(`/blogs/${selectedBlogId}/posts/${post.id}`, {
			token,
			method: 'PATCH',
			body: { status: post.status === 'published' ? 'draft' : 'published' }
		});
		busy = false;
		if (!result.ok) {
			postsError = result.error;
			return;
		}
		await selectBlog(selectedBlogId);
	}

	async function deletePost(post) {
		if (!post.deleted_warned) {
			post.deleted_warned = true;
			postsError = `Delete “${post.slug}” again to confirm. Its slug stays reserved.`;
			return;
		}
		busy = true;
		const result = await request(`/blogs/${selectedBlogId}/posts/${post.id}`, {
			token,
			method: 'DELETE'
		});
		busy = false;
		if (!result.ok) {
			postsError = result.error;
			return;
		}
		await selectBlog(selectedBlogId);
	}

	async function signOut() {
		await request('/auth/logout', { token });
		storeToken(null);
		token = null;
		me = null;
		blogs = [];
		selectedBlogId = null;
		posts = [];
		editor = null;
	}

	function formatTimestamp(unixSeconds) {
		if (!unixSeconds) return '—';
		return new Date(unixSeconds * 1000).toISOString().slice(0, 16).replace('T', ' ');
	}
</script>

<svelte:head>
	<title>Statesu — CMS dashboard</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<header class="header">
	<a class="brand" href="/">Statesu</a>
	{#if me}
		<nav aria-label="Session">
			<span>{me.email}</span>
			<button type="button" class="link" onclick={signOut}>Sign out</button>
		</nav>
	{/if}
</header>

<main>
	{#if !me}
		<section class="narrow">
			<h1>Sign in</h1>
			<p>Manage your blogs and Markdown posts from here. Browsers without a session see the <a href="/">API documentation</a>.</p>
			<form class="stack" onsubmit={submitAuth}>
				<label for="auth-email">Email</label>
				<input id="auth-email" type="email" autocomplete="email" bind:value={authEmail} required />
				<label for="auth-password">Password</label>
				<input id="auth-password" type="password" autocomplete="current-password" bind:value={authPassword} required />
				<div class="row">
					<button type="submit" disabled={authBusy}>{authMode === 'login' ? 'Sign in' : 'Create account'}</button>
					<button type="button" class="link" onclick={() => { authMode = authMode === 'login' ? 'register' : 'login'; authError = ''; }}>
						{authMode === 'login' ? 'Need an account? Register' : 'Already registered? Sign in'}
					</button>
				</div>
				{#if authError}<p class="error" role="alert">{authError}</p>{/if}
			</form>
		</section>
	{:else}
		<section class="workspace" aria-label="Blogs">
			<div class="panel">
				<h2>Blogs</h2>
				{#if blogs.length === 0}
					<p>No blogs yet. Create your first one:</p>
				{:else}
					<ul class="blog-list">
						{#each blogs as blog (blog.id)}
							<li>
								<button
									type="button"
									class="blog-link"
									aria-current={blog.id === selectedBlogId ? 'true' : undefined}
									onclick={() => selectBlog(blog.id)}
								>
									{blog.name}
									<code>/public/blogs/{blog.slug}</code>
								</button>
							</li>
						{/each}
					</ul>
				{/if}
				<form class="stack" onsubmit={createBlog}>
					<label for="blog-name">New blog name</label>
					<input id="blog-name" bind:value={newBlogName} placeholder="Writing desk" required />
					<label for="blog-slug">New blog slug</label>
					<input id="blog-slug" bind:value={newBlogSlug} placeholder="writing-desk" pattern="[a-z0-9]+(-[a-z0-9]+)*" required />
					<div><button type="submit">Create blog</button></div>
					{#if newBlogError}<p class="error" role="alert">{newBlogError}</p>{/if}
				</form>
			</div>

			{#if selectedBlog}
				<div class="panel">
					<h2>{selectedBlog.name}</h2>
					<p>Public API: <code>/public/blogs/{selectedBlog.slug}</code> — published posts are readable by anyone; drafts stay private.</p>

					<h3>Domain and allowed origins</h3>
					<p>Your site's exact origin(s) may call the public API from JavaScript. One origin per line: <code>scheme://host[:port]</code>, no paths, exact match. Punycode hosts and IPv6 literals like <code>[::1]:5173</code> are allowed; ports 1–65535. The domain is only metadata.</p>
					<form class="stack" onsubmit={(e) => { e.preventDefault(); saveSettings(); }}>
						<label for="blog-domain">Domain (optional)</label>
						<input id="blog-domain" bind:value={domainDraft} placeholder="example.com" />
						<label for="blog-origins">Allowed origins</label>
						<textarea id="blog-origins" rows="3" bind:value={originsDraft} placeholder={'https://example.com\nhttp://localhost:5173'}></textarea>
						<div><button type="submit">Save settings</button></div>
						{#if settingsSaved}<p class="ok">Settings saved.</p>{/if}
						{#if settingsError}<p class="error" role="alert">{settingsError}</p>{/if}
					</form>

					<h3>Posts</h3>
					{#if postsError}<p class="error" role="alert">{postsError}</p>{/if}
					<div class="row">
						<button type="button" onclick={startDraftPost}>New post</button>
					</div>
					{#if posts.length === 0}
						<p>Nothing here yet.</p>
					{:else}
						<div class="table-scroll">
							<table>
								<thead><tr><th>Slug</th><th>Title</th><th>Status</th><th>Published</th><th></th></tr></thead>
								<tbody>
									{#each posts as post (post.id)}
										<tr>
											<td><code>{post.slug}</code></td>
											<td>{post.title}</td>
											<td><span class="pill">{post.status}</span></td>
											<td>{formatTimestamp(post.published_at)}</td>
											<td class="actions">
												<button type="button" class="link" onclick={() => startEditPost(post)}>Edit</button>
												<button type="button" class="link" onclick={() => togglePublish(post)} disabled={busy}>
													{post.status === 'published' ? 'Withdraw' : 'Publish'}
												</button>
												<button type="button" class="link" onclick={() => deletePost(post)}>Delete</button>
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}

					{#if editor}
						<div class="editor">
							<h3>{editor.id ? 'Edit post' : 'New post'}</h3>
							<form class="stack" onsubmit={(e) => { e.preventDefault(); saveEditor(); }}>
								<div class="row">
									<span>
										<label for="post-slug">Slug</label>
										<input id="post-slug" bind:value={editor.slug} pattern="[a-z0-9]+(-[a-z0-9]+)*" required />
									</span>
									<span>
										<label for="post-title">Title</label>
										<input id="post-title" bind:value={editor.title} required />
									</span>
								</div>
								<label for="post-content">Markdown</label>
								<textarea id="post-content" rows="18" bind:value={editor.content_markdown} placeholder={'# Heading\n\nBody text…'}></textarea>
								<div class="row">
									<button type="submit" disabled={busy}>{editor.id ? 'Save changes' : 'Create draft'}</button>
									<button type="button" class="link" onclick={() => (editor = null)}>Cancel</button>
								</div>
								{#if editorError}<p class="error" role="alert">{editorError}</p>{/if}
							</form>
						</div>
					{/if}
				</div>
			{:else}
				<div class="panel">
					<h2>Welcome</h2>
					<p>Create a blog to start publishing Markdown.</p>
				</div>
			{/if}
		</section>
	{/if}
</main>

<style>
	.narrow { padding-block: 40px 200px; }
	.narrow form { margin-block: 24px; }
	.workspace { display: grid; gap: 32px; padding-block: 32px 200px; grid-template-columns: minmax(220px, 280px) 1fr; }
	@media (max-width: 720px) { .workspace { grid-template-columns: 1fr; } }
	.panel { border-top: 1px solid var(--border); padding-top: 8px; min-width: 0; }
	.stack { display: grid; gap: 6px; margin-block: 8px; }
	.stack input, .stack textarea, .editor input, .editor textarea {
		font: inherit; padding: 8px 10px; border: 1px solid var(--border); border-radius: 4px; background: #fff; color: inherit; width: 100%;
	}
	.stack textarea, .editor textarea { font-family: var(--mono); font-size: 13px; resize: vertical; }
	.row { display: flex; gap: 16px; align-items: center; margin-top: 10px; flex-wrap: wrap; }
	.row span { display: grid; gap: 4px; min-width: 0; }
	.row input { min-width: 120px; }
	button { font: inherit; padding: 8px 18px; border: 1px solid #171717; border-radius: 4px; background: #171717; color: #fff; cursor: pointer; }
	button.link { background: none; border: 0; color: inherit; padding: 0; text-decoration: underline; text-underline-offset: 4px; }
	button:disabled { opacity: .45; cursor: default; }
	.error { color: #171717; background: #f2f2f2; padding: 10px 12px; border-left: 3px solid #171717; }
	.ok { color: #171717; }
	.blog-list { list-style: none; padding: 0; margin: 0 0 24px; display: grid; gap: 6px; }
	.blog-link { display: block; text-align: left; width: 100%; background: none; color: inherit; border: 1px solid var(--border); border-radius: 4px; padding: 10px 12px; }
	.blog-link code { display: block; font-size: 11px; color: #6e6e6e; }
	.blog-link[aria-current='true'] { border-color: #171717; box-shadow: inset 3px 0 0 #171717; font-weight: 600; }
	.pill { display: inline-block; border: 1px solid #171717; border-radius: 999px; padding: 1px 10px; font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
	.actions { display: flex; gap: 14px; white-space: nowrap; }
	.editor { margin-top: 32px; border-top: 1px solid var(--border); padding-top: 24px; }
</style>
