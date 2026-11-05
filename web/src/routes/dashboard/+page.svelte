<script>
	import { isEmail, request } from '#lib/api.js';
	import { isValidPostPath, originFromSiteAddress } from '#lib/integration.js';
	import Turnstile from '#lib/components/Turnstile.svelte';

	// The Turnstile site key is baked in at build time (web/.env.example);
	// an empty key keeps the credential forms widget-free.
	const turnstileSiteKey = import.meta.env.PUBLIC_TURNSTILE_SITE_KEY || '';


	// Session ----------------------------------------------------------------
	/** @type {{ id: string, email: string } | null} */
	let me = $state(null);
	let authMode = $state('login');
	let authEmail = $state('');
	let authPassword = $state('');
	let authError = $state('');
	let authBusy = $state(false);
	let turnstileToken = $state('');
	/** @type {{ reset(): void } | null} */
	let challenge = $state(null);
	let notice = $state(''); // register → "check your inbox" state
	let canResend = $state(false);

	// Personal access tokens -------------------------------------------------
	/** @type {{ id: string, name: string, scopes: string, created_at: number, expires_at: number|null, last_used_at: number|null, delete_warned?: boolean }[]} */
	let pats = $state([]);
	let patName = $state('');
	let patWrite = $state(false);
	let patNeverExpires = $state(true);
	let patExpiry = $state('');
	let patError = $state('');
	/** @type {null | { id: string, name: string, scopes: string, created_at: number, expires_at: number|null, secret: string }} */
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
	/** @type {string|null} */
	let deleteBlogId = $state(null);
	let blogDeleteError = $state('');
	let blogDeleteBusy = $state(false);

	// Blog settings ----------------------------------------------------------
	let domainDraft = $state('');
	let originsDraft = $state('');
	let settingsError = $state('');
	let settingsSaved = $state(false);

	// Website integration ----------------------------------------------------
	let integrationOriginDraft = $state('');
	let integrationPath = $state('/blog');
	let integrationCopied = $state(false);
	let integrationNote = $state('');
	let integrationOriginValid = $derived.by(() => integrationOrigin() !== '');
	let integrationPathValid = $derived(validateIntegrationPath(integrationPath.trim()));
	let integrationSnippet = $derived.by(() => {
		if (!selectedBlog || !integrationPathValid) return '';
		return [
			'<div id="statesu-post"></div>',
			'<script defer',
			`  src="${location.origin}/embed/v1.js"`,
			`  data-blog="${selectedBlog.slug}"`,
			`  data-base-path="${integrationPath.trim()}"`,
			'  data-target="statesu-post"><\/script>'
		].join('\n');
	});
	let integrationExampleUrl = $derived.by(() => {
		const origin = integrationOrigin();
		if (!origin || !integrationPathValid) return '';
		const prefix = integrationPath.trim() === '/' ? '' : integrationPath.trim();
		return `${origin}${prefix}/${posts[0]?.slug ?? 'post-slug'}`;
	});

	function integrationOrigin() {
		return originFromSiteAddress(integrationOriginDraft);
	}

	function validateIntegrationPath(path) {
		return isValidPostPath(path);
	}

	async function copyIntegration() {
		integrationNote = '';
		integrationCopied = false;
		const snippet = integrationSnippet;
		try {
			await navigator.clipboard.writeText(snippet);
			integrationCopied = true;
		} catch {
			integrationNote = 'Copy failed — select the code below and copy it manually.';
		}
	}

	/** Adds the integration origin to the origins draft; Save applies it. */
	function allowIntegrationOrigin() {
		integrationNote = '';
		integrationCopied = false;
		const origin = integrationOrigin();
		if (!origin) {
			integrationNote = 'Enter your site address first.';
			return;
		}
		const existing = originsDraft.split('\n').map((line) => line.trim()).filter(Boolean);
		if (!existing.some((line) => line.toLowerCase() === origin.toLowerCase())) {
			originsDraft = [...existing, origin].sort().join('\n');
		}
		integrationNote = 'Origin added to the list — press Save settings to apply.';
	}

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

	// Only the CLI authorization route may be resumed after browser sign-in.
	function resumeCliAuthorization() {
		const next = new URLSearchParams(window.location.search).get('next');
		if (next && next.startsWith('/cli/authorize?')) {
			const target = new URL(next, window.location.origin);
			if (target.origin === window.location.origin && target.pathname === '/cli/authorize') {
				window.location.replace(target.href);
				return true;
			}
		}
		return false;
	}

	async function checkSession() {
		const result = await request('/auth/me');
		if (result.ok) {
			me = result.data;
			if (resumeCliAuthorization()) return;
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
		const body = { email: authEmail.trim(), password: authPassword };
		if (turnstileSiteKey) {
			if (!turnstileToken) {
				authError = 'Complete the human-verification challenge before continuing.';
				return;
			}
			body.turnstile = turnstileToken;
		}
		authBusy = true;
		const result = await request(`/auth/${authMode}`, { method: 'POST', body });
		authBusy = false;
		// Tokens are single-use: whatever the answer, the next attempt needs a
		// fresh challenge.
		challenge?.reset();
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
		if (resumeCliAuthorization()) return;
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
		const expiresAt = patNeverExpires ? null : Math.floor(new Date(patExpiry).getTime() / 1000);
		if (expiresAt !== null && (!Number.isFinite(expiresAt) || expiresAt <= Date.now() / 1000)) {
			patError = 'Choose an expiration date and time in the future.';
			return;
		}
		patBusy = true;
		const result = await request('/auth/pats', {
			method: 'POST',
			body: { name: patName.trim(), write: patWrite, expires_at: expiresAt }
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
		patNeverExpires = true;
		patExpiry = '';
		await refreshPats();
	}

	/** @param {{ id: string, name: string, scopes: string, created_at: number, expires_at: number|null, last_used_at: number|null, delete_warned?: boolean }} pat */
	async function deletePat(pat) {
		if (!pat.delete_warned) {
			pat.delete_warned = true;
			patError = `Confirm revocation of “${pat.name}”. Clients using it stop working immediately.`;
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
		const nextId = blogs.some((blog) => blog.id === selectedBlogId)
			? selectedBlogId : blogs[0]?.id ?? null;
		await selectBlog(nextId, view);
	}

	/** @param {string|null} id @param {'blogs'|'posts'|'settings'|'tokens'} nextView */
	async function selectBlog(id, nextView = 'posts') {
		selectedBlogId = id;
		view = nextView;
		deleteBlogId = null;
		blogDeleteError = '';
		posts = [];
		postsError = '';
		editor = null;
		editorError = '';
		settingsError = '';
		settingsSaved = false;
		if (!id) return;
		domainDraft = selectedBlog?.domain ?? '';
		originsDraft = (selectedBlog?.origins ?? []).join('\n');
		integrationOriginDraft = domainDraft;
		integrationOriginValid &&= true;
		integrationNote = '';
		integrationCopied = false;
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

	/** @param {{ id: string }} blog */
	async function deleteBlog(blog) {
		if (blogDeleteBusy) return;
		if (deleteBlogId !== blog.id) {
			deleteBlogId = blog.id;
			blogDeleteError = '';
			return;
		}
		blogDeleteBusy = true;
		blogDeleteError = '';
		const result = await request(`/blogs/${blog.id}`, { method: 'DELETE' });
		blogDeleteBusy = false;
		if (!result.ok) {
			if (signedOut(result)) return;
			blogDeleteError = result.error;
			return;
		}
		view = 'blogs';
		deleteBlogId = null;
		await listBlogs();
	}

	async function removeDomain() {
		if (!selectedBlogId) return;
		settingsError = '';
		settingsSaved = false;
		const result = await request(`/blogs/${selectedBlogId}`, {
			method: 'PATCH', body: { domain: null }
		});
		if (!result.ok) {
			if (signedOut(result)) return;
			settingsError = result.error;
			return;
		}
		blogs = blogs.map((blog) => blog.id === result.data.id ? result.data : blog);
		domainDraft = '';
		settingsSaved = true;
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
		await listBlogs();
		settingsSaved = true;
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

<main class="dashboard">
{#if !me}
	<section class="narrow">
		<a class="auth-brand" href="/">Statesu / Dashboard</a>
		<h1>{authMode === 'login' ? 'Welcome back' : 'Create your account'}</h1>
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
			{#if turnstileSiteKey}
				<Turnstile bind:token={turnstileToken} bind:this={challenge} sitekey={turnstileSiteKey} />
			{/if}
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
			<div class="menu-label">Workspace</div>
			<nav class="items" aria-label="Sections">
				{#each VIEWS as item (item.id)}
					<button
						type="button"
						class="item"
						aria-current={view === item.id ? 'page' : undefined}
						onclick={() => openView(item.id)}
					>
						<span>{item.label}</span>
						{#if item.id === 'blogs'}<span class="nav-count">{blogs.length}</span>{/if}
					</button>
				{/each}
			</nav>
			<div class="menu-footer">
				<a href="/">API documentation ↗</a>
				<button type="button" class="link signout" onclick={signOut}>Sign out</button>
			</div>
		</aside>

		<section class="content" aria-label="Dashboard content">
			<header class="content-header">
				<span class="eyebrow">Dashboard / {VIEWS.find((item) => item.id === view)?.label}</span>
				{#if selectedBlog && (view === 'posts' || view === 'settings')}
					<span class="current-blog">{selectedBlog.name}</span>
				{/if}
			</header>
			{#if view === 'blogs'}
				<div class="view">
					<h2>Blogs</h2>
					<p>One blog holds its own posts and delivery rules. Click one to work on it.</p>
					{#if blogs.length === 0}
						<p>No blogs yet. Create your first one below:</p>
					{:else}
						<div class="table-scroll">
							<table class="blogs-table" aria-label="Blogs">
								<thead><tr><th scope="col">Blog</th><th scope="col">Slug</th><th scope="col">Domain</th><th scope="col">Actions</th></tr></thead>
								<tbody>
									{#each blogs as blog (blog.id)}
										<tr>
											<td><button type="button" class="link blog-name" onclick={() => selectBlog(blog.id)}>{blog.name}</button></td>
											<td><code>{blog.slug}</code></td>
											<td>{blog.domain ?? '—'}</td>
											<td class="actions">
												<button type="button" class="link" aria-label={`Settings for ${blog.name}`} onclick={() => selectBlog(blog.id, 'settings')}>Settings</button>
												<button type="button" class="link" aria-label={`${deleteBlogId === blog.id ? 'Confirm deletion of' : 'Delete'} ${blog.name}`} disabled={blogDeleteBusy} onclick={() => deleteBlog(blog)}>
													{deleteBlogId === blog.id ? 'Confirm deletion' : 'Delete'}
												</button>
											</td>
										</tr>
										{#if deleteBlogId === blog.id}
											<tr class="delete-confirmation"><td colspan="4">
												<p role="alert">Permanently delete “{blog.name}”, all its posts, domain and allowed origins? This cannot be undone.</p>
												<button type="button" class="link" disabled={blogDeleteBusy} onclick={() => { deleteBlogId = null; blogDeleteError = ''; }}>Cancel</button>
											</td></tr>
										{/if}
									{/each}
								</tbody>
							</table>
						</div>
					{/if}
					{#if blogDeleteError}<p class="error" role="alert">{blogDeleteError}</p>{/if}
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
					<p>{#if selectedBlog}Public API: <code>/public/blogs/{selectedBlog.slug}</code> — {/if}Published posts are readable by anyone; drafts stay private.</p>
					{#if !selectedBlog}
						<p class="note">Create a blog in <button type="button" class="link" onclick={() => openView('blogs')}>Blogs</button> first.</p>
					{:else}
						<div class="toolbar">
							<div class="blog-select">
							<label for="post-blog">Blog</label>
							<select id="post-blog" bind:value={selectedBlogId} onchange={() => selectBlog(selectedBlogId)}>
								{#each blogs as blog (blog.id)}
									<option value={blog.id}>{blog.name}</option>
								{/each}
							</select>
							</div>
							<button type="button" onclick={startDraftPost}>+ New post</button>
						</div>
						{#if postsError}<p class="error" role="alert">{postsError}</p>{/if}
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
							{#if selectedBlog.domain}
								<div><button type="button" class="link" onclick={removeDomain}>Remove domain</button></div>
							{/if}
							<p>Clear the domain and save to remove it, or use Remove domain. Allowed origins are managed separately.</p>
							<label for="blog-origins">Allowed origins</label>
							<textarea id="blog-origins" rows="4" bind:value={originsDraft} placeholder={'https://example.com\nhttp://localhost:5173'}></textarea>
							<div><button type="submit">Save settings</button></div>
							{#if settingsSaved}<p class="ok">Settings saved.</p>{/if}
							{#if settingsError}<p class="error" role="alert">{settingsError}</p>{/if}
							<div>
								<button type="button" disabled={!integrationOriginValid} onclick={allowIntegrationOrigin}>
									Add site origin to the list
								</button>
							</div>
						</form>
					{/if}
				</div>
				<div class="view">
					<h3>Website integration</h3>
					{#if !selectedBlog}
						<p class="note">Create a blog first.</p>
					{:else}
						<p>Connect your website once: every published post becomes one URL there. Your site serves one HTML template under the posts path, and the embed script below fetches and renders the content safely.</p>
						<label for="integration-origin">Website address</label>
						<input id="integration-origin" bind:value={integrationOriginDraft} placeholder="burakselcuk.me" />
						{#if integrationOriginDraft.trim() !== '' && !integrationOriginValid}
							<p class="error" role="alert">Enter a valid site address like burakselcuk.me or https://burakselcuk.me.</p>
						{/if}
						<label for="integration-path">Posts path</label>
						<input id="integration-path" bind:value={integrationPath} placeholder="/blog" />
						{#if !integrationPathValid}
							<p class="error" role="alert">The posts path must be a single lowercase segment (like /blog) or exactly /.</p>
						{/if}
						{#if integrationOriginDraft.trim() === ''}
							<p class="note">Enter the address of the site that should show these posts. If your address isn't the dashboard origin, add it to Allowed origins above.</p>
						{/if}
						{#if integrationExampleUrl}
							<p>Example: <code>{integrationExampleUrl}</code></p>
						{/if}
						<p class="note">The embed script is served by this Statesu deployment, so your site needs no extra API origin — only its own address allowed above. Your hosting must route the posts path to one HTML page containing the target element.</p>
						<div>
							<button type="button" disabled={!integrationSnippet || !integrationOriginValid} onclick={copyIntegration}>{integrationCopied ? 'Copied!' : 'Copy code'}</button>
						</div>
						{#if integrationNote}<p class="ok" role="status">{integrationNote}</p>{/if}
						<details>
							<summary>Copyable integration code</summary>
							<div class="details-body">
								<pre><code>{integrationSnippet}</code></pre>
							</div>
						</details>
					{/if}
				</div>
			{:else}
				<div class="view">
					<h2>API tokens</h2>
					<p>Personal access tokens let CLI tools and scripts call the content API on your behalf with <code>Authorization: Bearer …</code>. They are shown once, can have an expiration date or never expire, and can be revoked anytime. Browsers should use your session, never a token.</p>
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
											<td>{pat.expires_at === null ? 'Never' : formatTimestamp(pat.expires_at)}</td>
											<td>{formatTimestamp(pat.last_used_at)}</td>
											<td class="actions">
												<button type="button" class="link" onclick={() => deletePat(pat)}>{pat.delete_warned ? 'Confirm revocation' : 'Revoke'}</button>
												{#if pat.delete_warned}
													<button type="button" class="link" onclick={() => { pat.delete_warned = false; patError = ''; }}>Cancel</button>
												{/if}
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
							<input type="checkbox" bind:checked={patNeverExpires} />
							Never expires
						</label>
						{#if patNeverExpires}
							<p>This token stays valid until revoked. An expiration date is recommended for security.</p>
						{:else}
							<label for="pat-expiry">Expiration date and time (your local time)</label>
							<input id="pat-expiry" type="datetime-local" bind:value={patExpiry} required />
						{/if}
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
</main>

<style>
	/* Keep documentation layout rules out of the dashboard workspace. */
	.dashboard { width: min(100% - 96px, 1440px); margin-inline: auto; color: #000; background: #fff; }
	.dashboard p, .dashboard ul, .dashboard code { color: inherit; }
	.dashboard :focus-visible { outline: 2px solid #000; outline-offset: 5px; }
	.dashboard section { padding: 0; border: 0; }
	.content-header { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 12px; padding-bottom: 24px; margin-bottom: 32px; }
	.content-header .eyebrow { font-size: 10px; }
	.current-blog { font-size: 12px; font-weight: 600; padding: 4px 10px; overflow-wrap: anywhere; }
	.menu-label { margin: 8px 12px -12px; font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: .12em; }
	.nav-count { margin-left: auto; font-size: 11px; }
	.menu-footer { display: grid; gap: 14px; margin-top: 32px; padding: 20px 12px 0; font-size: 12px; }
	.menu-footer a { color: inherit; text-decoration: none; }
	.toolbar { display: flex; justify-content: space-between; align-items: end; gap: 20px; margin: 20px 0; }
	.blog-select { display: grid; gap: 8px; flex: 1; max-width: 320px; }
	.auth-brand { font-size: 12px; font-weight: 700; }
	.narrow h1 { font-size: clamp(28px, 4vw, 40px); margin: 28px 0 16px; }

	.shell {
		display: grid;
		grid-template-columns: 228px minmax(0, 1fr);
		gap: clamp(32px, 4vw, 64px);
		padding-block: 48px 96px;
		min-height: 100dvh;
		align-items: start;
	}
	.menu {
		position: sticky;
		top: 24px;
		display: flex;
		flex-direction: column;
		gap: 24px;
		min-width: 0;
		padding: 24px 12px 20px;
		background: #fff;
	}
	.identity { display: grid; gap: 6px; padding-inline: 12px; }
	.identity .brand { font-weight: 700; letter-spacing: .08em; text-transform: uppercase; font-size: 13px; color: inherit; text-decoration: none; }
	.email { font-size: 12px; word-break: break-all; }
	.items { display: grid; gap: 2px; }
	.item {
		display: flex; align-items: center; gap: 8px;
		width: 100%; text-align: left;
		background: none; border: 0; border-radius: 0;
		padding: 11px 12px;
		font: inherit; color: inherit; cursor: pointer;
	}
	.item:hover { text-decoration: underline; text-underline-offset: 4px; }
	.item[aria-current='page'] { background: #000; color: #fff; font-weight: 600; }
	.signout { margin-top: auto; text-align: left; }
	.content { min-width: 0; padding-block: 8px; }
	.view { display: grid; gap: 16px; align-content: start; }
	.view + .view { margin-top: 32px; padding-top: 28px; }
	.view h2 { margin: 0; font-size: 30px; }
	.view h3 { margin: 20px 0 0; font-size: 18px; }
	.view p { margin: 0; font-size: 14px; line-height: 1.8; }
	.view > h3 { padding-top: 24px; }
	.view .mt { margin-top: 28px; }

	.dashboard .narrow { max-width: 480px; margin: 80px auto; padding: 32px; }
	.narrow p { font-size: 14px; }
	.narrow form { margin-top: 28px; }
	.stack { display: grid; gap: 10px; margin-block: 8px; max-width: 640px; }
	.stack > label:not(:first-child) { margin-top: 12px; }
	label { font-size: 12px; font-weight: 600; }
	.editor .stack { max-width: none; }
	.stack input, .stack textarea, .editor input, .editor textarea, select {
		font: inherit; font-size: 14px; padding: 11px 14px; border: 0; border-radius: 0; background: #000; color: #fff; width: 100%; min-width: 0; accent-color: #000;
	}
	input::placeholder, textarea::placeholder { color: #fff; opacity: 1; }
	.check input { appearance: none; width: 16px; height: 16px; padding: 0; flex: 0 0 16px; background: #000; cursor: pointer; }
	.check input:checked::after { content: '✓'; display: block; color: #fff; font-size: 12px; line-height: 16px; text-align: center; }
	.stack textarea, .editor textarea { font-family: var(--mono); font-size: 13px; resize: vertical; }
	.row { display: flex; flex-wrap: wrap; gap: 16px; align-items: center; margin-block: 12px 6px; }
	.row.wide { align-items: start; }
	.row.wide span { display: grid; gap: 4px; min-width: 0; flex: 1; }
	.check { display: flex; gap: 8px; align-items: center; }
	.check input { width: auto; }
	button { font: inherit; font-size: 13px; font-weight: 600; line-height: 1.6; padding: 11px 18px; border: 0; border-radius: 0; background: #000; color: #fff; cursor: pointer; }
	button:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 4px; }
	button.link { background: none; border: 0; color: inherit; padding: 0; text-decoration: underline; text-underline-offset: 4px; }
	button:disabled { cursor: not-allowed; text-decoration: line-through; }
	.view .error, .narrow .error { color: #fff; background: #000; padding: 10px 12px; }
	.ok { font-weight: 600; }
	.secret { display: inline-block; font-family: var(--mono); font-size: 13px; padding: 6px 10px; word-break: break-all; }
	.blogs-table { width: 100%; text-align: left; }
	.blogs-table th, .blogs-table td { vertical-align: top; border-bottom: 1px solid #ddd; }
	.blogs-table td:not(.actions) { overflow-wrap: anywhere; max-width: 280px; }
	.blog-name { text-align: left; }
	.delete-confirmation td { background: #f5f5f5; }
	.delete-confirmation button { margin-top: 8px; }
	.narrow a { color: inherit; text-decoration: underline; text-underline-offset: 4px; }
	.identity .brand { text-decoration: none; }
	.pill { display: inline-block; padding: 1px 10px; font-size: 10px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; }
	.actions { white-space: nowrap; }
	.actions button + button { margin-left: 14px; }
	.editor { margin-top: 32px; padding-top: 24px; }
	.table-scroll { overflow-x: auto; margin: 8px 0; }
	table { font-size: 12px; }
	th, td { padding: 16px; border: 0; }
	th { color: #000; font-size: 10px; }
	td:first-child { min-width: 140px; }
	td:nth-child(2) { white-space: normal; min-width: 160px; }

	/* Keep navigation readable without squeezing the content on smaller screens. */
	@media (max-width: 900px) {
		.dashboard { width: calc(100% - 48px); }
		.shell { grid-template-columns: 1fr; gap: 28px; padding-block: 24px 64px; }
		.menu-label { display: none; }
		.menu {
			position: static;
			z-index: 10;
			flex-direction: row;
			flex-wrap: wrap;
			align-items: center;
			gap: 12px;
			padding: 10px 12px;
		}
		.identity { flex: 1; }
		.items { display: flex; flex-wrap: nowrap; gap: 4px; width: 100%; overflow-x: auto; }
		.item { width: auto; flex: 1 0 auto; justify-content: center; white-space: nowrap; }
		.menu-footer { display: flex; margin: 0; padding: 4px 12px; border: 0; width: 100%; justify-content: space-between; }
		.identity .brand { font-size: 12px; }
		.signout { margin-top: 0; white-space: nowrap; }
	}
	@media (max-width: 540px) {
		.dashboard { width: calc(100% - 32px); }
		.shell { padding-top: 16px; gap: 24px; }
		.content-header { margin-bottom: 24px; padding-bottom: 16px; }
		.dashboard .narrow { margin-block: 24px; padding: 24px; }
		.row.wide { flex-direction: column; align-items: stretch; }
		.toolbar { flex-wrap: wrap; align-items: stretch; }
		.blog-select { max-width: none; flex-basis: 100%; }
		.toolbar > button { width: 100%; }
		.item { padding-inline: 10px; font-size: 12px; }
	}
</style>
