<script>
	import { isEmail, request, requestRaw } from '#lib/api.js';
	import { isValidPostPath, originFromSiteAddress } from '#lib/integration.js';
	import { buildMediaRenderer } from '../../embed/media-renderer.js';
	import { removeMediaImages } from '#lib/media.js';
	import Turnstile from '#lib/components/Turnstile.svelte';
	import EmbedAppearance from '#lib/components/EmbedAppearance.svelte';

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
		// v2 renders inside a shadow root and applies the blog's saved embed
		// appearance; v1 stays servable but is not recommended for new setups.
		return [
			'<div id="statesu-post"></div>',
			'<script defer',
			`  src="${location.origin}/embed/v2.js"`,
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

	// Media ------------------------------------------------------------------
	/** @type {{ id: string, mime: string, status: string, width: number|null, height: number|null, stored_bytes: number, public_url: string|null, variants: { label: string, public_url: string, width: number|null, height: number|null }[], delete_warned?: boolean }[]} */
	let media = $state([]);
	let mediaError = $state('');
	let uploading = $state(false);
	let uploadingName = $state('');
	let deletingMediaId = $state(null);
	let mediaNote = $state('');
	/** @type {{ id: string, account_id: string, bucket: string, prefix: string|null, public_base_url: string, name: string|null, delete_warned?: boolean }[]} */
	let connections = $state([]);
	let storagePreference = $state({ storage: 'statesu', connection_id: null });
	let storageError = $state('');
	let storageNote = $state('');
	/** @type {{ name: string, account_id: string, bucket: string, prefix: string, public_base_url: string, access_key_id: string, secret_key: string }} */
	let connectionDraft = $state({
		name: '', account_id: '', bucket: '', prefix: '',
		public_base_url: '', access_key_id: '', secret_key: ''
	});

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
		media = [];
		mediaError = '';
		mediaNote = '';
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
		await Promise.all([refreshMedia(), refreshStorage()]);
	}

	// Media helpers ---------------------------------------

	async function refreshMedia() {
		mediaError = '';
		if (!selectedBlogId) {
			media = [];
			return;
		}
		const result = await request(`/blogs/${selectedBlogId}/media?page=1&size=50`);
		if (!result.ok) {
			if (signedOut(result)) return;
			mediaError = result.error;
			return;
		}
		media = result.data.items;
	}

	function mediaByUrlMap() {
		const map = new Map();
		for (const entry of media) {
			if (typeof entry.public_url !== 'string' || entry.public_url === '') continue;
			const variants = entry.variants ?? [];
			map.set(entry.public_url, {
				source_url: entry.public_url,
				source_width: entry.width,
				source_height: entry.height,
				zoom_url: variants.length
					? variants[variants.length - 1].public_url
					: entry.public_url,
				variants: variants.map((variant) => ({
					label: variant.label,
					url: variant.public_url,
					width: variant.width,
					height: variant.height
				}))
			});
		}
		return map;
	}

	// Derived, not written-in-effect: every recompute produces plain data,
	// nothing recurses, and the preview reacts to editor/media together.
	const mediaByUrl = $derived(mediaByUrlMap());
	const previewRenderer = $derived(buildMediaRenderer(window, mediaByUrl));
	const previewHtml = $derived(
		editor && previewRenderer
			? previewRenderer(/** @type {{ content_markdown: string }} */ (editor).content_markdown)
			: ''
	);

	/**
	 * @param {File} file
	 * @param {{ blogId: string, editor: unknown, caret: number|null }} target
	 */
	async function uploadMediaFile(file, target) {
		uploading = true;
		uploadingName = file.name;
		try {
			const result = await requestRaw(`/blogs/${target.blogId}/media`, { body: file });
			if (!result.ok) {
				if (signedOut(result)) return;
				mediaError = result.error;
				return;
			}
			mediaError = '';
			// Everything the result publishes belongs to the blog and the
			// editor it was requested from: a meanwhile-switched blog's
			// media list never sees this entry, and a closed or switched
			// editor never receives text.
			if (selectedBlogId !== target.blogId || editor !== target.editor) return;
			media = [result.data ?? media[0], ...media].filter(Boolean);
			insertClipboardMarkdownAt(target.editor, target.caret, markdownForMedia(result.data ?? {}, file.name));
		} finally {
			uploading = false;
			uploadingName = '';
		}
	}

	/**
	 * @param {FileList | Array<File>|null|undefined} [fileList]
	 */
	async function uploadMediaFiles(fileList) {
		if (uploading || deletingMediaId || !selectedBlogId || !editor) return;
		mediaError = '';
		mediaNote = '';
		const area = /** @type {HTMLTextAreaElement | null} */ (document.getElementById('post-content'));
		const target = {
			blogId: /** @type {string} */ (selectedBlogId),
			editor,
			caret: area ? area.selectionStart : null
		};
		for (const file of [...(fileList ?? [])]) {
			if (!file.type.startsWith('image/')) {
				mediaError = `'${file.name}' is not recognized as an image by the browser.`;
				continue;
			}
			await uploadMediaFile(file, target);
			// Preserve file order instead of inserting every file at the original caret.
			target.caret = area?.selectionStart ?? null;
		}
	}

	/** @param {Record<string, unknown>} media @param {string} fileName */
	function markdownForMedia(media, fileName) {
		const url = typeof media.public_url === 'string' ? media.public_url : '';
		if (!url) return '';
		const alt = (fileName.split('.')[0] ?? 'image').trim().replace(/[\[\]]/g, '') || 'image';
		return `![${alt}](${url})`;
	}

	/** @param {string} markdown Inserts one Markdown line at the live caret. */
	function insertClipboardMarkdown(markdown) {
		if (!editor || !markdown) return;
		const area = /** @type {HTMLTextAreaElement | null} */ (document.getElementById('post-content'));
		insertClipboardMarkdownAt(editor, area?.selectionStart ?? null, markdown);
	}

	/**
	 * @param {unknown} targetEditor Inserts one Markdown line into exactly
	 *   that editor object at the given text position. Insertion is always
	 *   single-point: end and start split at the same index, so a caret
	 *   that moved since the snapshot can never delete ranges.
	 * @param {number|null} caret
	 * @param {string} markdown
	 */
	function insertClipboardMarkdownAt(targetEditor, caret, markdown) {
		if (targetEditor !== editor || !markdown || !editor) return;
		const area = /** @type {HTMLTextAreaElement | null} */ (document.getElementById('post-content'));
		const start = typeof caret === 'number'
			? Math.min(caret, editor.content_markdown.length)
			: (area?.selectionStart ?? editor.content_markdown.length);
		const before = editor.content_markdown.slice(0, start);
		const after = editor.content_markdown.slice(start);
		const glue = before && !before.endsWith('\n') ? '\n\n' : '';
		editor.content_markdown = `${before}${glue}${markdown}\n${after}`;
		let placed = before.length + glue.length + markdown.length;
		queueMicrotask(() => {
			if (area) {
				area.selectionStart = area.selectionEnd = placed + 1;
				area.focus();
			}
		});
	}

	/** @param {DragEvent} event */
	function onEditorDrop(event) {
		event.preventDefault();
		uploadMediaFiles(event.dataTransfer?.files ?? []);
	}

	/** @param {ClipboardEvent} event */
	function onEditorPaste(event) {
		const files = [...(event.clipboardData?.items ?? [])]
			.filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
			.map((item) => item.getAsFile())
			.filter((file) => file !== null);
		if (files.length === 0) return;
		event.preventDefault();
		uploadMediaFiles(files);
	}

	/** @param {{ id: string, public_url: string|null, variants?: { public_url: string }[], delete_warned?: boolean }} mediaEntry */
	async function deleteMediaEntry(mediaEntry) {
		if (!selectedBlogId || deletingMediaId || uploading) return;
		if (!mediaEntry.delete_warned) {
			for (const entry of media) entry.delete_warned = false;
			mediaEntry.delete_warned = true;
			mediaError = '';
			mediaNote = '';
			return;
		}
		const blogId = selectedBlogId;
		deletingMediaId = mediaEntry.id;
		try {
			const result = await request(`/blogs/${blogId}/media/${mediaEntry.id}`, { method: 'DELETE' });
			if (signedOut(result) || selectedBlogId !== blogId) return;
			if (!result.ok) {
				mediaError = result.error;
				return;
			}
			const urls = [mediaEntry.public_url, ...(mediaEntry.variants ?? []).map((variant) => variant.public_url)];
			if (editor) editor.content_markdown = removeMediaImages(editor.content_markdown, urls);
			mediaError = '';
			media = media.filter((entry) => entry.id !== mediaEntry.id);
			mediaNote = 'Image deleted. References in this editor were removed; save the post to keep those changes. Other saved posts are not updated.';
		} finally {
			deletingMediaId = null;
		}
	}

	// Storage connections -------------------------------------

	function emptyConnectionDraft() {
		return {
			name: '', account_id: '', bucket: '', prefix: '',
			public_base_url: '', access_key_id: '', secret_key: ''
		};
	}


	async function refreshStorage() {
		storageError = '';
		if (!selectedBlogId) {
			connections = [];
			storagePreference = { storage: 'statesu', connection_id: null };
			return;
		}
		const [listed, preference] = await Promise.all([
			request(`/blogs/${selectedBlogId}/storage-connections`),
			request(`/blogs/${selectedBlogId}/media-storage`)
		]);
		if (!listed.ok || !preference.ok) {
			if (signedOut(listed) || signedOut(preference)) return;
			storageError = listed.error ?? preference.error ?? '';
			return;
		}
		connections = listed.data;
		storagePreference = preference.data;
	}

	/** @param {SubmitEvent} event */
	async function createConnection(event) {
		event.preventDefault();
		storageError = '';
		storageNote = '';
		const body = {
			name: connectionDraft.name.trim() || null,
			account_id: connectionDraft.account_id.trim(),
			bucket: connectionDraft.bucket.trim(),
			prefix: connectionDraft.prefix.trim() || null,
			public_base_url: connectionDraft.public_base_url.trim().replace(/\/+$/, ''),
			access_key_id: connectionDraft.access_key_id.trim(),
			secret_key: connectionDraft.secret_key
		};
		const result = await request(`/blogs/${selectedBlogId}/storage-connections`, { method: 'POST', body });
		if (!result.ok) {
			if (signedOut(result)) return;
			storageError = result.error;
			return;
		}
		connectionDraft = emptyConnectionDraft();
		storageNote = 'Bucket verified with a probe object and connection created.';
		await refreshStorage();
	}

	/** @param {{ id: string, delete_warned?: boolean }} connection */
	async function deleteConnection(connection) {
		if (!selectedBlogId) return;
		if (!connection.delete_warned) {
			connection.delete_warned = true;
			storageError = `Delete again to confirm. Media uploaded to this bucket keeps living there.`;
			return;
		}
		const result = await request(`/blogs/${selectedBlogId}/storage-connections/${connection.id}`, { method: 'DELETE' });
		if (!result.ok) {
			if (signedOut(result)) return;
			storageError = result.error;
			return;
		}
		storageError = '';
		storageNote = '';
		await refreshStorage();
	}


	async function useServerStorage() {
		const result = await request(`/blogs/${selectedBlogId}/media-storage`, {
			method: 'PUT',
			body: { connection_id: null }
		});
		if (!result.ok) {
			if (signedOut(result)) return;
			storageError = result.error;
			return;
		}
		await refreshStorage();
	}

	/** @param {{ id: string }} connection */
	async function useCustomStorage(connection) {
		const result = await request(`/blogs/${selectedBlogId}/media-storage`, {
			method: 'PUT',
			body: { connection_id: connection.id }
		});
		if (!result.ok) {
			if (signedOut(result)) return;
			storageError = result.error;
			return;
		}
		await refreshStorage();
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

	/**
	 * Persists one appearance draft from the Embed appearance editor.
	 * Returns the error string for the editor, or null on success.
	 * @param {unknown} draft — wire-shaped appearance or null to clear
	 * @returns {Promise<string | null>}
	 */
	async function saveAppearance(draft) {
		if (!selectedBlogId) return 'No blog selected.';
		const result = await request(`/blogs/${selectedBlogId}/appearance`, {
			method: 'PUT',
			body: { embed_appearance: draft }
		});
		if (!result.ok) {
			if (signedOut(result)) return result.error;
			return result.error;
		}
		blogs = blogs.map((blog) => (blog.id === result.data.id ? result.data : blog));
		return null;
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
		<p>Sign in to manage your blogs and posts.</p>
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
			<nav class="items" aria-label="Account">
				{#each VIEWS.filter((item) => !item.needsBlog) as item (item.id)}
					<button
						type="button"
						class="item"
						aria-current={view === item.id ? 'page' : undefined}
						onclick={() => openView(item.id)}
					>
						<span>{item.label}</span>
						{#if item.id === 'blogs'}<span class="nav-count" aria-hidden="true">{blogs.length}</span>{/if}
					</button>
				{/each}
			</nav>
			{#if selectedBlog}
				<div class="blog-workspace">
					<div class="blog-switcher">
						<label for="active-blog">Blog workspace</label>
						<div class="blog-switcher-control">
							<select id="active-blog" bind:value={selectedBlogId} onchange={() => selectBlog(selectedBlogId, view)}>
								{#each blogs as blog (blog.id)}<option value={blog.id}>{blog.name}</option>{/each}
							</select>
							<svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="m5 6 3-3 3 3M5 10l3 3 3-3" stroke="currentColor" stroke-width="1.5" /></svg>
						</div>
					</div>
					<nav class="items" aria-label={`Blog: ${selectedBlog.name}`}>
						{#each VIEWS.filter((item) => item.needsBlog) as item (item.id)}
							<button
								type="button"
								class="item"
								aria-current={view === item.id ? 'page' : undefined}
								onclick={() => openView(item.id)}
							>
								{item.label}
							</button>
						{/each}
					</nav>
				</div>
			{/if}
			<div class="menu-footer">
				<a href="/docs/api">API documentation ↗</a>
				<button type="button" class="link signout" onclick={signOut}>Sign out</button>
			</div>
		</aside>

		<section class="content" aria-label="Dashboard content">
			<header class="content-header">
				<div>
					<h1>{VIEWS.find((item) => item.id === view)?.label}</h1>
					<p class="page-description">{view === 'blogs' ? 'Manage your blogs and choose one to work on.' : view === 'posts' ? 'Write, edit, and publish your posts.' : view === 'settings' ? 'Configure your blog, website, and embedded posts.' : 'Manage access for CLI tools and scripts.'}</p>
				</div>
			</header>
			{#if view === 'blogs'}
				<div class="view">
					{#if blogs.length === 0}
						<div class="empty-state"><strong>No blogs yet</strong><p>Create your first blog below to start writing.</p></div>
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
					<section class="panel" aria-labelledby="create-blog-heading">
					<h2 id="create-blog-heading">New blog</h2>
					<form class="stack" onsubmit={createBlog} aria-label="Create a blog">
						<label for="blog-name">Name</label>
						<input id="blog-name" bind:value={newBlogName} placeholder="Writing desk" required />
						<label for="blog-slug">Slug</label>
						<input id="blog-slug" bind:value={newBlogSlug} placeholder="writing-desk" pattern="[a-z0-9]+(-[a-z0-9]+)*" required />
						<div><button type="submit">Create blog</button></div>
						{#if newBlogError}<p class="error" role="alert">{newBlogError}</p>{/if}
					</form>
					</section>
				</div>
			{:else if view === 'posts'}
				<div class="view">

					{#if !selectedBlog}
						<p class="note">Create a blog in <button type="button" class="link" onclick={() => openView('blogs')}>Blogs</button> first.</p>
					{:else}
						<div class="toolbar">
							<span class="list-summary">{posts.length} {posts.length === 1 ? 'post' : 'posts'}</span>
							<button type="button" onclick={startDraftPost}>New post</button>
						</div>
						{#if postsError}<p class="error" role="alert">{postsError}</p>{/if}
						{#if posts.length === 0}
							<div class="empty-state"><strong>No posts yet</strong><p>Create a post to start writing. Drafts stay private until you publish them.</p></div>
						{:else}
							<div class="table-scroll">
								<table>
									<thead><tr><th scope="col">Post</th><th scope="col">Status</th><th scope="col">Published</th><th scope="col">Actions</th></tr></thead>
									<tbody>
										{#each posts as post (post.id)}
											<tr>
												<td><button type="button" class="link post-title" onclick={() => startEditPost(post)}>{post.title}</button><code class="post-slug">/{post.slug}</code></td>
												<td><span class="pill" class:published={post.status === 'published'}>{post.status}</span></td>
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
								<h2>{editor.id ? 'Edit post' : 'New post'}</h2>
								<form class="stack" onsubmit={(e) => { e.preventDefault(); saveEditor(); }}>
									<div class="row wide">
										<span><label for="post-slug">Slug</label>
											<input id="post-slug" bind:value={editor.slug} pattern="[a-z0-9]+(-[a-z0-9]+)*" required /></span>
										<span><label for="post-title">Title</label>
											<input id="post-title" bind:value={editor.title} required /></span>
									</div>
									<label for="post-content">Markdown</label>
									<div class="media-toolbar">
										<button type="button" class="link media-button" disabled={uploading || !!deletingMediaId} onclick={() => document.getElementById('media-pick')?.click()}>Add images…</button>
										<input
											id="media-pick"
											type="file"
											accept="image/jpeg,image/png,image/webp"
											multiple
											disabled={uploading || !!deletingMediaId}
											hidden
											onchange={(event) => {
												{ uploadMediaFiles(/** @type {HTMLInputElement} */ (event.currentTarget).files); }
												event.currentTarget.value = '';
											}}
										/>
										<span class="note">…or drop / paste an image onto the editor. The file name becomes the alt text.</span>
										{#if uploading}<p class="note" role="status">Uploading {uploadingName}…</p>{/if}
										{#if mediaError}<p class="error" role="alert">{mediaError}</p>{/if}
										{#if mediaNote}<p class="note" role="status">{mediaNote}</p>{/if}
									</div>
									<textarea
										id="post-content"
										rows="18"
										bind:value={editor.content_markdown}
										ondrop={onEditorDrop}
										ondragover={(event) => event.preventDefault()}
										onpaste={onEditorPaste}
										placeholder={'# Heading\n\nBody text…'}
									></textarea>
									{#if previewHtml}
										<div class="preview mt">
											<h4>Preview</h4>
											<div class="statesu-body">{@html previewHtml}</div>
										</div>
									{/if}
									<div class="row">
										<button type="submit" disabled={busy || uploading || !!deletingMediaId}>{busy ? 'Saving…' : editor.id ? 'Save changes' : 'Create draft'}</button>
										<button type="button" class="link" onclick={() => (editor = null)}>Cancel</button>
									</div>
											<div class="media-list">
												{#if media.length === 0}
													<span class="note">No media in this blog yet.</span>
												{:else}
													<span class="note">Media — {media.length}</span>
													<ul>
														{#each media as mediaEntry (mediaEntry.id)}
															<li>
																{#if mediaEntry.public_url}<img class="media-thumbnail" src={mediaEntry.public_url} alt="" loading="lazy" />{/if}
																<code>{mediaEntry.mime}</code>
																{mediaEntry.width ?? '?'}×{mediaEntry.height ?? '?'} · {Math.round(mediaEntry.stored_bytes / 1024)} KiB
																<button type="button" class="link" disabled={uploading || !!deletingMediaId || !mediaEntry.public_url} onclick={() => insertClipboardMarkdown(markdownForMedia(mediaEntry, mediaEntry.id))}>Insert</button>
																<button type="button" class="link" disabled={uploading || !!deletingMediaId} onclick={() => deleteMediaEntry(mediaEntry)}>{deletingMediaId === mediaEntry.id ? 'Deleting…' : mediaEntry.delete_warned ? 'Confirm delete' : 'Delete'}</button>
																{#if mediaEntry.delete_warned}
																	<button type="button" class="link" disabled={!!deletingMediaId} onclick={() => (mediaEntry.delete_warned = false)}>Cancel deletion</button>
																	<p class="media-warning">Permanently delete this image? References in this editor will be removed. Other saved posts may show a broken image.</p>
																{/if}
															</li>
														{/each}
													</ul>
												{/if}
											</div>
									{#if editorError}<p class="error" role="alert">{editorError}</p>{/if}
								</form>
							</div>
						{/if}
					{/if}
				</div>
			{:else if view === 'settings'}
				<div class="view">
					{#if !selectedBlog}
						<p class="note">Create a blog in <button type="button" class="link" onclick={() => openView('blogs')}>Blogs</button> first.</p>
					{:else}
						<nav class="section-nav" aria-label="Settings sections">
							<a href="#delivery">Domain & origins</a><a href="#storage">Media storage</a><a href="#integration">Website integration</a><a href="#appearance">Appearance</a>
						</nav>
						<section class="panel" id="delivery" aria-labelledby="delivery-heading">
						<h2 id="delivery-heading">Domain and allowed origins</h2>
						<p>Choose which websites can read your posts through the public API.</p>
						<form class="stack" onsubmit={(e) => { e.preventDefault(); saveSettings(); }}>
							<label for="blog-domain">Domain (optional)</label>
							<input id="blog-domain" bind:value={domainDraft} placeholder="example.com" />
							{#if selectedBlog.domain}
								<div><button type="button" class="link" onclick={removeDomain}>Remove domain</button></div>
							{/if}
							<p class="field-help">For reference only. Leave blank to remove; this does not change allowed origins.</p>
							<label for="blog-origins">Allowed origins</label>
							<textarea id="blog-origins" rows="4" bind:value={originsDraft} aria-describedby="origins-help" placeholder={'https://example.com\nhttp://localhost:5173'}></textarea>
							<p class="field-help" id="origins-help">One exact origin per line, including https:// or http:// and an optional port. No paths.</p>
							<div><button type="submit">Save settings</button></div>
							{#if settingsSaved}<p class="ok">Settings saved.</p>{/if}

							{#if settingsError}<p class="error" role="alert">{settingsError}</p>{/if}
						</form>
						</section>
						<section class="panel" id="storage" aria-labelledby="storage-heading">
						<h2 id="storage-heading">Media storage</h2>
							<p>Choose where new images are uploaded. Existing images stay where they are.</p>
							{#if storageError}<p class="error" role="alert">{storageError}</p>{/if}
							{#if storageNote}<p class="ok" role="status">{storageNote}</p>{/if}
							<p>
								In use: <code>{storagePreference.connection_id ? 'your R2 bucket' : "Statesu's bucket"}</code>
								<button type="button" class="link" onclick={useServerStorage} disabled={storagePreference && !storagePreference.connection_id}>Use Statesu's bucket</button>
							</p>
							{#if connections.length > 0}
								<div class="table-scroll">
									<table>
										<thead><tr><th scope="col">Bucket</th><th scope="col">Prefix</th><th scope="col">Public URL</th><th scope="col">Actions</th></tr></thead>
										<tbody>
											{#each connections as connection (connection.id)}
												<tr>
													<td>{connection.bucket}</td>
													<td>{connection.prefix ?? ''}</td>
													<td><code>{connection.public_base_url}</code></td>
													<td class="actions">
														<button type="button" class="link" onclick={() => useCustomStorage(connection)} disabled={storagePreference.connection_id === connection.id}>Use</button>
														<button type="button" class="link" onclick={() => deleteConnection(connection)}>{connection.delete_warned ? 'Confirm delete' : 'Delete'}</button>
													</td>
												</tr>
											{/each}
										</tbody>
									</table>
								</div>
							{/if}
							<details>
								<summary>Connect your own R2 bucket</summary>
								<div class="details-body">
									<aside class="note" aria-labelledby="r2-security-heading">
										<strong id="r2-security-heading">Security: connect a dedicated media bucket</strong>
										<p>Statesu stores your secret access key encrypted and decrypts it on the server to upload and delete media on your behalf. Credentials are never returned in API responses.</p>
										<p>Use R2 S3 credentials with <strong>Object Read &amp; Write access to this bucket only</strong>. Never provide your Cloudflare Global API Key or account-wide credentials. A key prefix does not restrict what the credentials can access.</p>
										<p>Use a bucket dedicated to public media, not private or unrelated files. Uploaded media is public; your bucket needs a public domain.</p>
										<p>You can revoke these credentials in Cloudflare at any time. Revoking them stops future uploads and cleanup; already-public media may remain accessible.</p>
									</aside>
									<form class="stack" onsubmit={(event) => { event.preventDefault(); void createConnection(event); }}>
										<label for="conn-name">Name (optional)</label>
										<input id="conn-name" bind:value={connectionDraft.name} placeholder="Blog photos" />
										<label for="conn-account">Cloudflare account ID (32 hex characters)</label>
										<input id="conn-account" bind:value={connectionDraft.account_id} size="32" required />
										<label for="conn-bucket">Bucket name</label>
										<input id="conn-bucket" bind:value={connectionDraft.bucket} required />
										<label for="conn-prefix">Object key prefix (optional, e.g. statesu)</label>
										<input id="conn-prefix" bind:value={connectionDraft.prefix} />
										<label for="conn-public">Public base URL (https://media.example.com)</label>
										<input id="conn-public" bind:value={connectionDraft.public_base_url} required />
										<label for="conn-key">Access key ID</label>
										<input id="conn-key" bind:value={connectionDraft.access_key_id} required />
										<label for="conn-secret">R2 secret access key</label>
										<input id="conn-secret" type="password" bind:value={connectionDraft.secret_key} required />
										<button type="submit">Verify and connect</button>
									</form>
								</div>
							</details>
						</section>
					{/if}
				</div>
				{#if selectedBlog}
				<section class="panel" id="integration" aria-labelledby="integration-heading">
					<h2 id="integration-heading">Website integration</h2>
						<p>Show published posts on your website with a single embed snippet.</p>
						<p><strong>SEO on your own domain:</strong> The embed loads posts after JavaScript runs. To serve post content and metadata in the initial HTML, set up <a href="/docs/integration#search">server-side rendering (SSR) or static export</a> on your website. No setting is required in Statesu.</p>
						<div class="stack">
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
							<p class="field-help">Enter the website where you want your posts to appear.</p>
						{/if}
						{#if integrationExampleUrl}
							<p>Example: <code>{integrationExampleUrl}</code></p>
						{/if}
						<div><button type="button" class="secondary" disabled={!integrationOriginValid} onclick={allowIntegrationOrigin}>Add site origin to the list</button></div>
						<p class="field-help">Then use Save settings in Domain and allowed origins to apply the change.</p>
						</div>
						<p>Your hosting must route all URLs under the posts path to an HTML page containing this snippet.</p>
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
				</section>
				<section class="panel" id="appearance" aria-label="Embed appearance">
					<EmbedAppearance blog={selectedBlog} save={saveAppearance} />
				</section>
				{/if}
			{:else}
				<div class="view">
					<p>Use tokens with <code>Authorization: Bearer …</code>. Keep them private; browser apps should use session sign-in instead.</p>
					{#if freshPat}
						<p class="ok">Token created. Copy the secret now — it will not be shown again:</p>
						<p><code class="secret">{freshPat.secret}</code></p>
						<div class="row"><button type="button" onclick={dismissFreshPat}>Done</button></div>
					{/if}
					{#if patError}<p class="error" role="alert">{patError}</p>{/if}
					{#if pats.length === 0}
						<div class="empty-state"><strong>No API tokens yet</strong><p>Create a token below to connect a CLI tool or script.</p></div>
					{:else}
						<div class="table-scroll">
							<table>
								<thead><tr><th scope="col">Name</th><th scope="col">Scopes</th><th scope="col">Created</th><th scope="col">Expires</th><th scope="col">Last used</th><th scope="col">Actions</th></tr></thead>
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
					<section class="panel" aria-labelledby="new-token-heading">
					<h2 id="new-token-heading">New token</h2>
					<p>Copy your token when it is created. The secret is only shown once.</p>
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
					</section>
				</div>
			{/if}
		</section>
	</div>
{/if}
</main>

<style>
	/* Media preview parity with the embed's own body rules: hosted files
	   never escape their container at any viewport width. */
	.preview :global(.statesu-body img) {
		max-width: 100%;
		height: auto;
	}

	/* Keep documentation layout rules out of the dashboard workspace. */
	.dashboard { width: min(100% - 64px, 1440px); margin-inline: auto; color: #000; background: #fff; font-size: 14px; line-height: 1.6; }
	.dashboard p, .dashboard ul, .dashboard code { color: inherit; }
	.dashboard p { max-width: 70ch; }
	.dashboard .note, .dashboard .field-help { font-size: 13px; line-height: 1.6; }
	/* Sections are separated by whitespace, not containers. */
	.dashboard .panel { display: grid; gap: 16px; padding: 0; margin-top: 48px; scroll-margin-top: 32px; min-width: 0; }
	.view > .section-nav + .panel { margin-top: 16px; }
	.panel > h2, .editor > h2 { margin: 0; font-size: 18px; font-weight: 650; letter-spacing: -.02em; }
	.panel > p { margin: 0; font-size: 14px; }
	.nav-count, .list-summary { font-size: 13px; font-variant-numeric: tabular-nums; }
	.section-nav { display: flex; flex-wrap: wrap; gap: 12px 24px; padding: 0; }
	.section-nav a { padding: 4px 0; font-size: 13px; text-decoration: underline; text-underline-offset: 4px; }
	.empty-state { display: grid; gap: 8px; padding: 24px 0; }
	.post-slug { display: block; margin-top: 4px; }
	.post-title { text-align: left; }
	.dashboard :focus-visible { outline: 2px solid #000; outline-offset: 5px; }
	.dashboard section { padding: 0; border: 0; }
	.content-header { display: flex; justify-content: space-between; align-items: start; flex-wrap: wrap; gap: 24px; padding-bottom: 0; margin-bottom: 40px; }
	.content-header h1 { margin: 0; font-size: 30px; font-weight: 650; line-height: 1.2; letter-spacing: -.035em; }
	.content-header .page-description { margin: 12px 0 0; font-size: 14px; }
	.nav-count { margin-left: auto; }
	.menu-footer { display: grid; gap: 14px; margin-top: 32px; padding: 20px 12px 0; font-size: 12px; }
	.menu-footer a { color: inherit; text-decoration: underline; }
	.toolbar { display: flex; justify-content: space-between; align-items: center; gap: 20px; margin: 0 0 8px; }
	.blog-workspace { display: grid; gap: 12px; min-width: 0; padding-top: 24px; border-top: 1px solid #ddd; }
	.blog-workspace .item { padding-left: 24px; }
	.blog-switcher { min-width: 0; padding: 0 12px; }
	.blog-switcher label { display: block; margin-bottom: 4px; font-size: 11px; font-weight: 500; letter-spacing: .08em; text-transform: uppercase; }
	.blog-switcher-control { position: relative; }
	.blog-switcher-control select { appearance: none; background: transparent; color: inherit; padding: 8px 24px 8px 0; font-weight: 600; cursor: pointer; text-overflow: ellipsis; }
	.blog-switcher-control select:hover { text-decoration: underline; text-underline-offset: 4px; }
	.blog-switcher-control svg { position: absolute; right: 0; top: 50%; transform: translateY(-50%); pointer-events: none; }
	@media (forced-colors: active) {
		.blog-switcher-control select { appearance: auto; }
		.blog-switcher-control svg { display: none; }
	}
	.auth-brand { font-size: 12px; font-weight: 700; }
	.narrow h1 { font-size: clamp(28px, 4vw, 40px); margin: 28px 0 16px; }

	.shell {
		display: grid;
		grid-template-columns: 220px minmax(0, 1fr);
		gap: clamp(32px, 4vw, 64px);
		padding-block: 32px 80px;
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
		padding: 8px 20px 24px 0;
		background: #fff;
	}
	.identity { display: grid; gap: 6px; padding: 0 12px 20px; }
	.identity .brand { font-weight: 750; letter-spacing: -.035em; font-size: 23px; color: inherit; text-decoration: none; }
	.email { font-size: 13px; overflow-wrap: anywhere; }
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
	.view h2 { margin: 0; font-size: 18px; font-weight: 650; }
	.view p { margin: 0; font-size: 14px; line-height: 1.6; }
	.view .mt { margin-top: 28px; }

	.dashboard .narrow { max-width: 480px; margin: 80px auto; padding: 32px; }
	.narrow p { font-size: 14px; }
	.narrow form { margin-top: 28px; }
	.stack { display: grid; gap: 10px; margin-block: 8px; max-width: 640px; }
	.stack > label:not(:first-child) { margin-top: 12px; }
	label { font-size: 13px; font-weight: 600; }
	.editor .stack { max-width: none; }
	.stack input, .stack textarea, .editor input, .editor textarea, select {
		font: inherit; font-size: 14px; line-height: 1.5; padding: 12px 14px; border: 0; border-radius: 0; background: #000; color: #fff; width: 100%; min-width: 0; accent-color: #000;
	}
	input::placeholder, textarea::placeholder { color: #fff; opacity: .7; }
	.stack .check input { width: 16px; height: 16px; padding: 0; flex: 0 0 16px; cursor: pointer; }
	.stack textarea, .editor textarea { font-family: var(--mono); font-size: 13px; resize: vertical; }
	.row { display: flex; flex-wrap: wrap; gap: 16px; align-items: center; margin-block: 12px 6px; }
	.row.wide { align-items: start; }
	.row.wide span { display: grid; gap: 4px; min-width: 0; flex: 1; }
	.check { display: flex; gap: 8px; align-items: center; }
	button { font: inherit; font-size: 13px; font-weight: 600; line-height: 1.5; padding: 12px 18px; border: 0; border-radius: 0; background: #000; color: #fff; cursor: pointer; min-height: 40px; }
	button:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 4px; }
	button.link, button.secondary { background: none; color: inherit; padding: 4px 0; min-height: 32px; text-decoration: underline; text-underline-offset: 4px; }
	button.link:hover:not(:disabled), button.secondary:hover:not(:disabled) { text-decoration-thickness: 2px; }
	button:disabled { cursor: not-allowed; opacity: .5; }
	.media-toolbar { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; }
	.media-toolbar p { flex-basis: 100%; }
	.media-list ul { list-style: none; padding: 0; display: grid; gap: 12px; }
	.media-list li { display: flex; flex-wrap: wrap; align-items: center; gap: 16px; padding: 12px 0; }
	.media-thumbnail { width: 64px; height: 64px; object-fit: contain; }
	.media-warning { flex-basis: 100%; }
	.dashboard .error { color: #fff; background: #000; padding: 12px 16px; }
	.ok { font-weight: 600; }
	.secret { display: inline-block; font-family: var(--mono); font-size: 13px; padding: 6px 10px; word-break: break-all; }
	.blogs-table { width: 100%; text-align: left; }
	.blogs-table th, .blogs-table td { vertical-align: top; }
	.blogs-table td:not(.actions) { overflow-wrap: anywhere; max-width: 280px; }
	.blog-name { text-align: left; }
	.delete-confirmation td { padding-top: 0; }
	.delete-confirmation button { margin-top: 8px; }
	.narrow a { color: inherit; text-decoration: underline; text-underline-offset: 4px; }
	.identity .brand { text-decoration: none; }
	.pill { display: inline-block; font-size: 13px; text-transform: capitalize; white-space: nowrap; }
	.pill.published { font-weight: 700; }
	.actions { white-space: nowrap; }
	.actions button + button { margin-left: 16px; }
	.editor { margin-top: 48px; padding: 0; }
	.table-scroll { overflow-x: auto; margin: 0; }
	table { font-size: 14px; }
	th, td { padding: 16px 24px 16px 0; border: 0; vertical-align: middle; }
	th:last-child, td:last-child { padding-right: 0; }
	th { color: #000; font-size: 13px; font-weight: 600; text-transform: none; letter-spacing: 0; white-space: nowrap; }
	td:first-child { min-width: 160px; }
	td:nth-child(2) { white-space: normal; min-width: 100px; }
	details { min-width: 0; }
	summary { padding: 8px 0; cursor: pointer; font-size: 13px; font-weight: 600; text-decoration: underline; text-underline-offset: 4px; }
	.details-body { padding: 16px 0 0; }
	pre { margin: 8px 0 0; padding: 0; overflow-x: auto; line-height: 1.7; }
	.preview { padding: 24px 0; }
	.preview h4 { margin: 0 0 16px; font-size: 14px; }

	/* Keep navigation readable without squeezing the content on smaller screens. */
	@media (max-width: 900px) {
		.dashboard { width: calc(100% - 48px); }
		.shell { grid-template-columns: 1fr; gap: 28px; padding-block: 24px 64px; }
		.menu {
			position: static;
			z-index: 10;
			flex-direction: row;
			flex-wrap: wrap;
			align-items: center;
			gap: 12px;
			padding: 0 0 20px;
		}
		.identity { flex: 1; padding: 0 0 8px; }
		.blog-workspace { width: 100%; padding-top: 16px; }
		.blog-workspace .item { padding-left: 12px; }
		.blog-switcher { width: min(220px, 100%); padding: 0; }
		.items { display: flex; flex-wrap: nowrap; gap: 4px; width: 100%; overflow-x: auto; }
		.item { width: auto; flex: 1 0 auto; justify-content: center; white-space: nowrap; }
		.menu-footer { display: flex; margin: 0; padding: 4px 12px; border: 0; width: 100%; justify-content: space-between; }
		.identity .brand { font-size: 22px; }
		.signout { margin-top: 0; white-space: nowrap; }
	}
	@media (max-width: 540px) {
		.dashboard { width: calc(100% - 32px); }
		.shell { padding-top: 16px; gap: 24px; }
		.content-header { margin-bottom: 24px; padding-bottom: 16px; }
		.dashboard .narrow { margin-block: 24px; padding: 16px 0; }
		.dashboard .panel, .editor { padding: 0; margin-top: 40px; }
		.blog-switcher { width: 100%; }
		.row.wide { flex-direction: column; align-items: stretch; }
		.toolbar { flex-wrap: wrap; align-items: stretch; }
		.toolbar > button { width: 100%; }
		.item { padding-inline: 10px; font-size: 12px; }
	}
</style>
