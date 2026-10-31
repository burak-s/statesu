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
	// Views ------------------------------------------------------------------
	/** @type {{ id: 'blogs'|'posts'|'settings'|'tokens', label: string, needsBlog?: boolean }[]} */
	const VIEWS = [
		{ id: 'blogs', label: 'Blogs' },
		{ id: 'posts', label: 'Posts', needsBlog: true },
		{ id: 'settings', label: 'Settings', needsBlog: true },
		{ id: 'tokens', label: 'API tokens' }
	];
	/** @type {'blogs'|'posts'|'settings'|'tokens'} */
	let view = $state('blogs');

	/** @param {'blogs'|'posts'|'settings'|'tokens'} next */
	function openView(next) {
		if ((next === 'posts' || next === 'settings') && !selectedBlogId && blogs.length) {
			selectedBlogId = blogs[0].id;
		}
		view = next;
		if (next === 'posts') {
			selectBlog(selectedBlogId);
		} else if (next === 'tokens') {
			refreshPats();
		}
	}

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
		view = 'posts';
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

{#if !me}
	<section class="narrow">
		<h1>Sign in</h1>
		<p>Manage your blogs and Markdown posts from here. Browsers without a session see the <a href="/">API documentation</a>.</p>
		{#if notice}
			<p class="ok" role="status">{notice}</p>
			{#if canResend}
				<button type="button" class="link" onclick={resendVerification} disabled={authBusy}>
					Resend the verification mail
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
	<div class="shell">
		<aside class="menu" aria-label="Dashboard navigation">
			<div class="identity">
				<a class="brand" href="/">Statesu</a>
				<span class="email">{me.email}</span>
			</div>
			<nav class="items" aria-label="Sections">
				{#each VIEWS as item (item.id)}
					<button
						type="button"
						class="item"
						aria-current={view === item.id ? 'page' : undefined}
						onclick={() => openView(item.id)}
					>
						{#if item.needsBlog && selectedBlog}<span class="bull">·</span>{/if}
						{item.label}
					</button>
				{/each}
			</nav>
			<button type="button" class="link signout" onclick={signOut}>Sign out</button>
		</aside>

		<section class="content">
			{#if view === 'blogs'}
				<div class="view">
					<h2>Blogs</h2>
					<p>One blog holds its own posts and delivery rules. Click one to work on it.</p>
					{#if blogs.length === 0}
						<p>No blogs yet. Create your first one below:</p>
					{:else}
						<ul class="blog-list">
							{#each blogs as blog (blog.id)}
								<li>
									<button type="button" class="blog-link" aria-current={blog.id === selectedBlogId ? 'true' : undefined} onclick={() => selectBlog(blog.id)}>
										{blog.name}
										<code>/public/blogs/{blog.slug}</code>
									</button>
								</li>
							{/each}
						</ul>
					{/if}
					<h3>New blog</h3>
					<form class="stack" onsubmit={createBlog} aria-label="Create a blog">
						<label for="blog-name">Name</label>
						<input id="blog-name" bind:value={newBlogName} placeholder="Writing desk" required />
						<label for="blog-slug">Slug</label>
						<input id="blog-slug" bind:value={newBlogSlug} placeholder="writing-desk" pattern="[a-z0-9]+(-[a-z0-9]+)*" required />
						<div><button type="submit">Create blog</button></div>
						{#if newBlogError}<p class="error" role="alert">{newBlogError}</p>{/if}
					</form>
				</div>
			{:else if view === 'posts'}
				<div class="view">
					<h2>Posts</h2>
					<p>Public API: <code>/public/blogs/{selectedBlog?.slug}</code> — published posts are readable by anyone; drafts stay private.</p>
					{#if !selectedBlog}
						<p class="note">Create a blog in <button type="button" class="link" onclick={() => openView('blogs')}>Blogs</button> first.</p>
					{:else}
							<label for="post-blog">Blog</label>
						<div class="row">
							<select id="post-blog" bind:value={selectedBlogId} onchange={() => selectBlog(selectedBlogId)}>
								{#each blogs as blog (blog.id)}
									<option value={blog.id}>{blog.name}</option>
								{/each}
							</select>
						</div>
						{#if postsError}<p class="error" role="alert">{postsError}</p>{/if}
						<div class="row"><button type="button" onclick={startDraftPost}>New post</button></div>
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
									<div class="row wide">
										<span><label for="post-slug">Slug</label>
											<input id="post-slug" bind:value={editor.slug} pattern="[a-z0-9]+(-[a-z0-9]+)*" required /></span>
										<span><label for="post-title">Title</label>
											<input id="post-title" bind:value={editor.title} required /></span>
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
					{/if}
				</div>
			{:else if view === 'settings'}
				<div class="view">
					<h2>Settings</h2>
					{#if !selectedBlog}
						<p class="note">Create a blog in <button type="button" class="link" onclick={() => openView('blogs')}>Blogs</button> first.</p>
					{:else}
						<p>{selectedBlog.name}</p>
						<h3>Domain and allowed origins</h3>
						<p>Your site's exact origin(s) may call the public API from JavaScript. One origin per line: <code>scheme://host[:port]</code>, no paths, exact match. Punycode hosts and IPv6 literals like <code>[::1]:5173</code> are allowed; ports 1–65535. The domain is only metadata.</p>
						<form class="stack" onsubmit={(e) => { e.preventDefault(); saveSettings(); }}>
							<label for="blog-domain">Domain (optional)</label>
							<input id="blog-domain" bind:value={domainDraft} placeholder="example.com" />
							<label for="blog-origins">Allowed origins</label>
							<textarea id="blog-origins" rows="4" bind:value={originsDraft} placeholder={'https://example.com\nhttp://localhost:5173'}></textarea>
							<div><button type="submit">Save settings</button></div>
							{#if settingsSaved}<p class="ok">Settings saved.</p>{/if}
							{#if settingsError}<p class="error" role="alert">{settingsError}</p>{/if}
						</form>
					{/if}
				</div>
			{:else}
				<div class="view">
					<h2>API tokens</h2>
					<p>Personal access tokens let CLI tools and scripts call the content API on your behalf with <code>Authorization: Bearer …</code>. They are shown once, expire, and can be revoked anytime. Browsers should use your session, never a token.</p>
				</div>
				<div class="view">
					{#if freshPat}
						<p class="ok">Token created. Copy the secret now — it will not be shown again:</p>
						<p><code class="secret">{freshPat.secret}</code></p>
						<div class="row"><button type="button" onclick={dismissFreshPat}>Done</button></div>
					{/if}
					{#if patError}<p class="error" role="alert">{patError}</p>{/if}
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
					<h3 class="mt">New token</h3>
					<form class="stack" onsubmit={createPat}>
						<label for="pat-name">Name</label>
						<input id="pat-name" bind:value={patName} placeholder="ci-script" maxlength="64" required />
						<label class="check">
							<input type="checkbox" bind:checked={patWrite} />
							Allow writing (create and edit blogs and posts)
						</label>
						<div><button type="submit" disabled={patBusy}>Create token</button></div>
					</form>
				</div>
			{/if}
		</section>
	</div>
{/if}

<style>
	/* The global stylesheet caps `main` at 920px for docs pages. The dashboard
	   ships its own CSS (this same rule only loads on this page) and is an
	   application, so it claims the full width minus comfortable gutters. */
	:global(main) { width: min(100% - 48px, 1480px); margin-inline: auto; }

	.shell {
		display: grid;
		grid-template-columns: 240px minmax(0, 1fr);
		gap: 48px;
		padding-block: 32px 200px;
		align-items: start;
	}
	.menu {
		position: sticky;
		top: 24px;
		display: flex;
		flex-direction: column;
		gap: 24px;
		min-width: 0;
		border: 1px solid var(--border);
		border-radius: 6px;
		padding: 20px;
		background: #fff;
	}
	.identity { display: grid; gap: 4px; }
	.identity .brand { font-weight: 700; letter-spacing: .08em; text-transform: uppercase; font-size: 13px; color: inherit; text-decoration: none; }
	.email { font-size: 12px; color: #6e6e6e; word-break: break-all; }
	.items { display: grid; gap: 2px; }
	.item {
		display: flex; align-items: center; gap: 8px;
		width: 100%; text-align: left;
		background: none; border: 0; border-radius: 4px;
		padding: 9px 12px;
		font: inherit; color: inherit; cursor: pointer;
	}
	.item:hover { background: #f5f5f5; }
	.item[aria-current='page'] { background: #171717; color: #fff; font-weight: 600; }
	.item .bull { opacity: .55; font-weight: 700; }
	.signout { margin-top: auto; text-align: left; }
	.content { min-width: 0; }
	.view { display: grid; gap: 6px; align-content: start; }
	.view h2 { margin: 0; margin-bottom: 6px; }
	.view h3 { margin: 18px 0 0; }
	.mt { margin-top: 28px; }

	.narrow { padding-block: 40px 200px; }
	.narrow form { margin-block: 24px; }
	.stack { display: grid; gap: 6px; margin-block: 8px; }
	.stack input, .stack textarea, .editor input, .editor textarea {
		font: inherit; padding: 8px 10px; border: 1px solid var(--border); border-radius: 4px; background: #fff; color: inherit; width: 100%;
	}
	select {
		font: inherit; padding: 8px 10px; border: 1px solid var(--border); border-radius: 4px; background: #fff; color: inherit;
	}
	.stack textarea, .editor textarea { font-family: var(--mono); font-size: 13px; resize: vertical; }
	.row { display: flex; gap: 16px; align-items: end; margin-block: 12px 6px; }
	.row.wide { align-items: start; }
	.row.wide span { display: grid; gap: 4px; min-width: 0; flex: 1; }
	.check { display: flex; gap: 8px; align-items: center; }
	.check input { width: auto; }
	button { font: inherit; padding: 8px 18px; border: 1px solid #171717; border-radius: 4px; background: #171717; color: #fff; cursor: pointer; }
	button.link { background: none; border: 0; color: inherit; padding: 0; text-decoration: underline; text-underline-offset: 4px; }
	button:disabled { opacity: .45; cursor: default; }
	.error { color: #171717; background: #f2f2f2; padding: 10px 12px; border-left: 3px solid #171717; }
	.ok { color: #171717; }
	.note { color: #6e6e6e; }
	.secret { display: inline-block; font-family: var(--mono); font-size: 13px; background: #f2f2f2; padding: 6px 10px; word-break: break-all; }
	.blog-list { list-style: none; padding: 0; margin: 0 0 24px; display: grid; gap: 6px; }
	.blog-link { display: block; text-align: left; width: 100%; background: none; color: inherit; border: 1px solid var(--border); border-radius: 4px; padding: 10px 12px; }
	.narrow a { color: inherit; text-decoration: underline; text-underline-offset: 4px; }
	.identity .brand { text-decoration: none; }
	.blog-link code { display: block; font-size: 11px; color: #6e6e6e; }
	.blog-link[aria-current='true'] { border-color: #171717; box-shadow: inset 3px 0 0 #171717; font-weight: 600; }
	.pill { display: inline-block; border: 1px solid #171717; border-radius: 999px; padding: 1px 10px; font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
	.actions { display: flex; gap: 14px; white-space: nowrap; }
	.editor { margin-top: 32px; border-top: 1px solid var(--border); padding-top: 24px; }
	.table-scroll { overflow-x: auto; }

	/* Mobile: the sidebar becomes a sticky horizontal tab strip. */
	@media (max-width: 900px) {
		:global(main) { width: calc(100% - 32px); }
		.shell { grid-template-columns: 1fr; gap: 16px; }
		.menu {
			position: sticky;
			top: 0;
			z-index: 10;
			flex-direction: row;
			align-items: center;
			gap: 12px;
			padding: 10px 12px;
		}
		.email { display: none; }
		.items { display: flex; gap: 4px; flex: 1; overflow-x: auto; }
		.identity .brand { font-size: 12px; }
		.signout { margin-top: 0; white-space: nowrap; }
	}
</style>
