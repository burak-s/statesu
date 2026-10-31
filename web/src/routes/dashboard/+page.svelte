<script>
	import { isEmail, request } from '#lib/api.js';

	// Session ----------------------------------------------------------------
	/** @type {{ id: string, email: string } | null} */
	let me = $state(null);
	let authMode = $state('login');
	let authEmail = $state('');
	let authPassword = $state('');
	let authError = $state('');
	let authBusy = $state(false);
	let notice = $state(''); // register → "check your inbox" state
	let canResend = $state(false);

	// Personal access tokens -------------------------------------------------
	/** @type {{ id: string, name: string, scopes: string, created_at: number, expires_at: number, last_used_at: number|null, delete_warned?: boolean }[]} */
	let pats = $state([]);
	let patName = $state('');
	let patWrite = $state(false);
	let patError = $state('');
	/** @type {null | { id: string, name: string, scopes: string, created_at: number, expires_at: number, secret: string }} */
	let freshPat = $state(null); // secret shown exactly once
	let patBusy = $state(false);

	// Blogs ------------------------------------------------------------------
	/** @type {any[]} */
	let blogs = $state([]);
	/** @type {string | null} */
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
	/** @type {{ id: string, slug: string, title: string, content_markdown: string, status: string, published_at: number|null, deleted_warned?: boolean }[]} */
	let posts = $state([]);
	let postsError = $state('');
	/** @type {{ id: null|string, slug: string, title: string, content_markdown: string, status: string, published_at?: number|null } | null} */
	let editor = $state(null);
	let editorError = $state('');
	let busy = $state(false);

	const selectedBlog = $derived(blogs.find((b) => b.id === selectedBlogId) ?? null);

	$effect(() => {
		if (typeof localStorage !== 'undefined' && localStorage.getItem('statesu_token')) {
			localStorage.removeItem('statesu_token'); // migrated away; one cleanup pass
		}
		checkSession();
	});

	async function checkSession() {
		const result = await request('/auth/me');
		if (result.ok) {
			me = result.data;
			await refreshPats();
			await refreshBlogs();
		} else {
			me = null;
		}
	}

	/** @param {{ unauthorized?: boolean }|undefined|null} result @returns {boolean} */
	function signedOut(result) {
		if (result?.unauthorized) {
			me = null;
			pats = [];
			blogs = [];
			selectedBlogId = null;
			posts = [];
			editor = null;
			return true;
		}
		return false;
	}

	/** @param {SubmitEvent} event */
	async function submitAuth(event) {
		event.preventDefault();
		authError = '';
		notice = '';
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
			if (signedOut(result)) return;
			authError = result.error;
			return;
		}
		if (authMode === 'register') {
			// No credential comes back: the address must be confirmed first.
			notice = 'Check your inbox — a verification link valid for 30 minutes is on its way.';
			canResend = true;
			authPassword = '';
			return;
		}
		me = result.data;
		authPassword = '';
		await refreshPats();
		await refreshBlogs();
	}

	async function resendVerification() {
		authError = '';
		authBusy = true;
		const result = await request('/auth/resend', {
			method: 'POST',
			body: { email: authEmail.trim() }
		});
		authBusy = false;
		if (result.ok) {
			notice = 'A fresh verification mail is on its way (if the address still needs it).';
			canResend = false; // the server throttles repeats
		} else {
			authError = result.error;
		}
	}

	// Personal access tokens -------------------------------------------------
	async function refreshPats() {
		const result = await request('/auth/pats');
		if (result.ok) pats = result.data;
		else signedOut(result);
	}

	/** @param {SubmitEvent} event */
	async function createPat(event) {
		event.preventDefault();
		patError = '';
		if (!patName.trim()) {
			patError = 'Give the token a name you will recognize later.';
			return;
		}
		patBusy = true;
		const result = await request('/auth/pats', {
			method: 'POST',
			body: { name: patName.trim(), write: patWrite }
		});
		patBusy = false;
		if (!result.ok) {
			if (signedOut(result)) return;
			patError = result.error;
			return;
		}
		freshPat = result.data; // `secret` is shown exactly once, now
		patName = '';
		patWrite = false;
		await refreshPats();
	}

	/** @param {{ id: string, name: string, scopes: string, created_at: number, expires_at: number, last_used_at: number|null, delete_warned?: boolean }} pat */
	async function deletePat(pat) {
		if (!pat.delete_warned) {
			pat.delete_warned = true;
			patError = `Delete “${pat.name}” again to confirm. Clients using it stop working immediately.`;
			refreshPats();
			return;
		}
		patError = '';
		const result = await request(`/auth/pats/${pat.id}`, { method: 'DELETE' });
		if (!result.ok && result.error !== 'token not found' && !signedOut(result)) {
			patError = result.error;
			return;
		}
		await refreshPats();
	}

	function dismissFreshPat() {
		freshPat = null;
		refreshPats();
	}

	// Blogs ------------------------------------------------------------------
	async function refreshBlogs() {
		const meResult = await request('/auth/me');
		if (signedOut(meResult)) return;
		if (meResult.ok) me = meResult.data;
		await listBlogs();
	}

	async function listBlogs() {
		const result = await request('/blogs');
		if (!result.ok) {
			if (signedOut(result)) return;
			newBlogError = result.error;
			return;
		}
		blogs = result.data;
		await selectBlog(selectedBlogId ?? blogs[0]?.id ?? null);
	}

	/** @param {string|null} id */
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
		const result = await request(`/blogs/${id}/posts`);
		if (!result.ok) {
			if (signedOut(result)) return;
			postsError = result.error;
			return;
		}
		posts = result.data.items;
	}

	/** @param {SubmitEvent} event */
	async function createBlog(event) {
		event.preventDefault();
		newBlogError = '';
		const result = await request('/blogs', {
			method: 'POST',
			body: { name: newBlogName.trim(), slug: newBlogSlug.trim() }
		});
		if (!result.ok) {
			if (signedOut(result)) return;
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
			method: 'PUT',
			body: { origins }
		});
		if (!originsResult.ok) {
			if (signedOut(originsResult)) return;
			settingsError = originsResult.error;
			return;
		}
		if (domain !== (selectedBlog?.domain ?? '')) {
			const patch = await request(`/blogs/${selectedBlogId}`, {
				method: 'PATCH',
				body: { domain: domain === '' ? null : domain }
			});
			if (!patch.ok) {
				if (signedOut(patch)) return;
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

	/** @param {{ id: string, slug: string, title: string, content_markdown: string, status: string, published_at: number|null, deleted_warned?: boolean }} post */
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
		if (!editor) return;
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
		const result = await request(path, { method: editor.id ? 'PATCH' : 'POST', body });
		busy = false;
		if (!result.ok) {
			if (signedOut(result)) return;
			editorError = result.error;
			return;
		}
		editor = null;
		await selectBlog(selectedBlogId);
	}

	/** @param {{ id: string, slug: string, title: string, content_markdown: string, status: string, published_at: number|null, deleted_warned?: boolean }} post */
	async function togglePublish(post) {
		busy = true;
		const result = await request(`/blogs/${selectedBlogId}/posts/${post.id}`, {
			method: 'PATCH',
			body: { status: post.status === 'published' ? 'draft' : 'published' }
		});
		busy = false;
		if (!result.ok) {
			if (signedOut(result)) return;
			postsError = result.error;
			return;
		}
		await selectBlog(selectedBlogId);
	}

	/** @param {{ id: string, slug: string, title: string, content_markdown: string, status: string, published_at: number|null, deleted_warned?: boolean }} post */
	async function deletePost(post) {
		if (!post.deleted_warned) {
			post.deleted_warned = true;
			postsError = `Delete “${post.slug}” again to confirm. Its slug stays reserved.`;
			return;
		}
		busy = true;
		const result = await request(`/blogs/${selectedBlogId}/posts/${post.id}`, {
			method: 'DELETE'
		});
		busy = false;
		if (!result.ok) {
			if (signedOut(result)) return;
			postsError = result.error;
			return;
		}
		await selectBlog(selectedBlogId);
	}

	async function signOut() {
		await request('/auth/logout', { method: 'POST' });
		me = null;
		pats = [];
		blogs = [];
		selectedBlogId = null;
		posts = [];
		editor = null;
	}

	/** @param {number|null|undefined} unixSeconds */
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
			{#if notice}
				<p class="ok" role="status">{notice}</p>
				{#if canResend}
					<button type="button" class="link" onclick={resendVerification} disabled={authBusy}>
						 resend the verification mail
					</button>
				{/if}
			{/if}
			<form class="stack" onsubmit={submitAuth}>
				<label for="auth-email">Email</label>
				<input id="auth-email" type="email" autocomplete="email" bind:value={authEmail} required />
				<label for="auth-password">Password</label>
				<input id="auth-password" type="password" autocomplete="current-password" bind:value={authPassword} required />
				<div class="row">
					<button type="submit" disabled={authBusy}>{authMode === 'login' ? 'Sign in' : 'Create account'}</button>
					<button type="button" class="link" onclick={() => { authMode = authMode === 'login' ? 'register' : 'login'; authError = ''; notice = ''; }}>
						{authMode === 'login' ? 'Need an account? Register' : 'Already registered? Sign in'}
					</button>
				</div>
				{#if authError === 'email not verified'}
					<p class="error" role="alert">This address is not verified yet.</p>
					<button type="button" class="link" onclick={resendVerification} disabled={authBusy}>
						Resend the verification mail
					</button>
				{:else if authError}
					<p class="error" role="alert">{authError}</p>
				{/if}
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

			<div class="panel">
				<h2>API tokens</h2>
				<p>Personal access tokens let CLI tools and scripts call the content API on your behalf with <code>Authorization: Bearer …</code>. They are shown once, expire, and can be revoked anytime. Browsers should use your session, never a token.</p>
				{#if freshPat}
					<p class="ok">Token created. Copy the secret now — it will not be shown again:</p>
					<p><code class="secret">{freshPat.secret}</code></p>
					<div class="row"><button type="button" onclick={dismissFreshPat}>Done</button></div>
				{/if}
				{#if pats.length === 0}
					<p>No tokens yet.</p>
				{:else}
					<div class="table-scroll">
						<table>
							<thead><tr><th>Name</th><th>Scopes</th><th>Created</th><th>Expires</th><th>Last used</th><th></th></tr></thead>
							<tbody>
								{#each pats as pat (pat.id)}
									<tr>
										<td>{pat.name}</td>
										<td><span class="pill">{pat.scopes}</span></td>
										<td>{formatTimestamp(pat.created_at)}</td>
										<td>{formatTimestamp(pat.expires_at)}</td>
										<td>{formatTimestamp(pat.last_used_at)}</td>
										<td class="actions">
											<button type="button" class="link" onclick={() => deletePat(pat)}>Revoke</button>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
				<form class="stack" onsubmit={createPat}>
					<label for="pat-name">New token name</label>
					<input id="pat-name" bind:value={patName} placeholder="ci-script" maxlength="64" required />
					<label class="check">
						<input type="checkbox" bind:checked={patWrite} />
						Allow writing (create and edit blogs and posts)
					</label>
					<div><button type="submit" disabled={patBusy}>Create token</button></div>
					{#if patError}<p class="error" role="alert">{patError}</p>{/if}
				</form>
			</div>
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
	.check { display: flex; gap: 8px; align-items: center; }
	.check input { width: auto; }
	button { font: inherit; padding: 8px 18px; border: 1px solid #171717; border-radius: 4px; background: #171717; color: #fff; cursor: pointer; }
	button.link { background: none; border: 0; color: inherit; padding: 0; text-decoration: underline; text-underline-offset: 4px; }
	button:disabled { opacity: .45; cursor: default; }
	.error { color: #171717; background: #f2f2f2; padding: 10px 12px; border-left: 3px solid #171717; }
	.ok { color: #171717; }
	.secret { display: inline-block; font-family: var(--mono); font-size: 13px; background: #f2f2f2; padding: 6px 10px; word-break: break-all; }
	.blog-list { list-style: none; padding: 0; margin: 0 0 24px; display: grid; gap: 6px; }
	.blog-link { display: block; text-align: left; width: 100%; background: none; color: inherit; border: 1px solid var(--border); border-radius: 4px; padding: 10px 12px; }
	.blog-link code { display: block; font-size: 11px; color: #6e6e6e; }
	.blog-link[aria-current='true'] { border-color: #171717; box-shadow: inset 3px 0 0 #171717; font-weight: 600; }
	.pill { display: inline-block; border: 1px solid #171717; border-radius: 999px; padding: 1px 10px; font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
	.actions { display: flex; gap: 14px; white-space: nowrap; }
	.editor { margin-top: 32px; border-top: 1px solid var(--border); padding-top: 24px; }
</style>
