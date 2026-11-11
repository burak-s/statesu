// Statesu embed v2: the same single-post / latest / feed behaviors as v1,
// with three upgrades:
//   1. The content lives inside one shadow root, so the host page CSS can
//      neither leak in nor be affected out.
//   2. Public delivery metadata (responsive variants, zoom surface) from
//      the post payload feeds the shared media renderer: trusted srcsets,
//      lazy loading, width pins via `{width=N}`.
//   3. Linked images keep the author's link; plain images open the
//      lightbox (disable with data-lightbox="off").
//
//   <script defer
//     src="https://statesu.example/embed/v2.js"
//     data-blog="burak"
//     data-base-path="/blog"
//     data-target="statesu-post"></script>
//
// v1 keeps serving pages until hosts pin v2 explicitly: no silent upgrade.

import { parseBasePath, resolveSlug } from './url.js';
import { parseApiOrigin, apiUrl } from './api.js';
import { buildMediaRenderer } from './media-renderer.js';
import { createLightbox } from './lightbox.js';
import {
	DEFAULT_TARGET_ID,
	FETCH_TIMEOUT_MS,
	describeFetchFailure,
	element,
	fetchPostJson,
	renderPost,
	showState,
	validBlogSlug
} from './common.js';

const LIBRARY_VERSION = 'v2';

/**
 * Public metadata map: source_url → { variants, source_width, zoom_url, … }.
 * Unknown entries simply yield plain <img> rendering.
 */
function mediaByUrlOf(window, payload) {
	const map = new Map();
	for (const media of Array.isArray(payload.media) ? payload.media : []) {
		if (typeof media?.source_url !== 'string') continue;
		// The renderer resolves relative hrefs to absolute ones; the public
		// payload only ever ships absolute URLs, so key directly on it.
		try {
			const key = new URL(media.source_url, window.location.href).href;
			map.set(key, media);
		} catch {
			// Unresolvable source URLs cannot gain srcsets.
		}
	}
	return map;
}

function stylesFor(version) {
	return `
.statesu-v2 { color: inherit; font-family: inherit; font-size: 1rem; line-height: 1.6; }
.statesu-v2 .statesu-state { opacity: .75; font-style: italic; }
.statesu-v2 .statesu-error { opacity: .85; }
.statesu-v2 .statesu-title { margin: 0 0 .5em; }
.statesu-v2 .statesu-body img { max-width: 100%; height: auto; display: block; margin: 1.25em auto; }
.statesu-v2 .statesu-body img.linked-in { display: inline; margin: 0; }
.statesu-v2 .statesu-body pre { overflow-x: auto; }
.statesu-v2 .statesu-body a { text-decoration: underline; }
.statesu-v2 .statesu-announcement + .statesu-announcement { margin-top: 2rem; padding-top: 2rem; border-top: 1px solid currentColor; }
.statesu-v2 .statesu-more { margin-top: 1.5rem; font: inherit; color: inherit; background: none; border: 0; padding: 0; text-decoration: underline; cursor: pointer; }
@media (prefers-reduced-motion: reduce) {
	.statesu-v2 .statesu-body img { transition: none; }
}`;
}

function injectShadowStyles(shadowOwnerDocument, shadow, version) {
	const style = shadowOwnerDocument.createElement('style');
	style.textContent = stylesFor(version);
	shadow.appendChild(style);
}

/**
 * Marks images that live inside an author link: the lightbox must never
 * steal those clicks.
 */
function annotateLinkedImages(root) {
	for (const anchor of root.querySelectorAll('a')) {
		for (const image of anchor.querySelectorAll('img')) {
			image.classList.add('linked-in');
		}
	}
}

/** Click-to-open plus keyboard parity: any non-linked image in the body. */
function attachLightboxStage(root, lightbox, enabled) {
	if (!enabled) return;
	const opensLightbox = (image) => {
		const zoom = image.dataset.zoomUrl || image.getAttribute('src') || '';
		lightbox.open(zoom, image.getAttribute('alt') ?? '', image);
	};
	for (const image of root.querySelectorAll('img')) {
		if (image.classList.contains('linked-in')) continue;
		image.setAttribute('tabindex', '0');
		image.setAttribute('role', 'button');
		image.setAttribute('aria-label', 'Open image preview');
		image.addEventListener('keydown', (event) => {
			if (event.key !== 'Enter' && event.key !== ' ') return;
			event.preventDefault();
			opensLightbox(image);
		});
	}
	root.addEventListener('click', (event) => {
		const image = event.target?.closest?.('img');
		if (!image || image.classList.contains('linked-in')) return;
		opensLightbox(image);
	});
}

/**
 * Attaches metadata to rendered images: dataset.zoomUrl for the lightbox
 * and refresh of the responsive map on re-render.
 */
function attachMediaMetadata(body, mediaByUrl) {
	if (!mediaByUrl?.size) return;
	for (const image of body.querySelectorAll('img')) {
		const source = image.getAttribute('src') ?? '';
		const meta = mediaByUrl.get(source);
		if (!meta) continue;
		if (meta.zoom_url) image.dataset.zoomUrl = meta.zoom_url;
	}
}

function ownScript(document, libraryVersion) {
	const current = document.currentScript;
	if (current && current.getAttribute('data-blog') !== null) return current;
	for (const script of document.querySelectorAll('script[data-blog]')) {
		const src = script.getAttribute('src') ?? '';
		if (src.endsWith(`/embed/${libraryVersion}.js`) || (current && script === current)) {
			return script;
		}
	}
	return document.querySelectorAll('script[data-blog]')[0] ?? null;
}

/**
 * Entry point for one page. Runs once; a second script tag is ignored.
 * `timeoutMs` exists for tests; production callers use the default.
 */
export function init(window, { timeoutMs = FETCH_TIMEOUT_MS } = {}) {
	const document = window.document;
	if (!document || window.__STATESU_EMBED_V2__) {
		if (window.__STATESU_EMBED_V2__) {
			window.console?.warn?.('Statesu embed v2 already initialized on this page.');
		}
		return;
	}
	window.__STATESU_EMBED_V2__ = true;

	const script = ownScript(document, LIBRARY_VERSION);
	if (!script) return;

	const targetId = script.dataset.target || DEFAULT_TARGET_ID;
	const target = document.getElementById(targetId);
	if (!target) {
		window.console?.error?.(`Statesu embed: target element #${targetId} not found.`);
		return;
	}

	const blog = script.dataset.blog;
	// One stable host keeps the shadow tree; the page only sees it mount.
	const host = element(document, 'div', 'statesu-v2-host');
	host.dataset.statesuV2Host = '';
	const shadow = host.attachShadow({ mode: 'open' });
	injectShadowStyles(document, shadow, LIBRARY_VERSION);
	const root = element(document, 'article', 'statesu-v2 statesu-embed');
	shadow.appendChild(root);
	// Lightbox panel attaches to the page, not to the shadow tree: dialogs
	// must escape host page overflow, styles stay inside their own root.
	const lightboxEnabled = script.getAttribute('data-lightbox') !== 'off';
	const lightbox = createLightbox(window, { disabled: !lightboxEnabled });

	const mount = () => {
		target.textContent = '';
		target.appendChild(host);
	};

	if (!validBlogSlug(blog)) {
		mount();
		showState(
			root,
			'Statesu embed: invalid or missing data-blog slug (lowercase letters, digits, hyphens).',
			true
		);
		return;
	}

	let basePath;
	try {
		basePath = parseBasePath(script.dataset['basePath']);
	} catch (error) {
		mount();
		showState(root, `Statesu embed: ${error.message}`, true);
		return;
	}

	const isLatest = script.dataset.mode === 'latest';
	const isFeed = script.dataset.mode === 'feed';
	const hideTitle = script.hasAttribute('data-hide-title');
	const fixedPost = script.getAttribute('data-post');
	const resolution = isLatest || isFeed
		? { kind: 'collection' }
		: fixedPost === null
		? resolveSlug(window.location.href, basePath)
		: validBlogSlug(fixedPost)
			? { kind: 'post', slug: fixedPost }
			: { kind: 'invalid' };
	if (resolution.kind === 'skip') return;

	mount();
	if (resolution.kind === 'empty') {
		showState(root, 'This page hosts blog posts. Open /blog/<slug> to read one.', false);
		return;
	}
	if (resolution.kind === 'invalid') {
		showState(root, 'This URL is not a valid blog post address.', true);
		return;
	}

	const configured = parseApiOrigin(script.getAttribute('src') ?? '');
	if (!configured.ok) {
		showState(
			root,
			configured.reason === 'insecure'
				? 'Statesu embed: the API must be served over HTTPS (except local development).'
				: 'Statesu embed: the script address does not define a usable API origin.',
			true
		);
		return;
	}

	/**
	 * Renders one validated post payload into the shadow root, wiring the
	 * metadata renderer and the lightbox stage.
	 */
	function render(post) {
		const mediaByUrl = mediaByUrlOf(window, post);
		const render = buildMediaRenderer(window, mediaByUrl) ?? null;
		root.textContent = '';
		renderPost(window, root, post, render, hideTitle);
		attachMediaMetadata(root, mediaByUrl);
		annotateLinkedImages(root);
		attachLightboxStage(root, lightbox, lightboxEnabled);
	}

	if (isFeed) {
		initFeed(window, root, configured.origin, blog, hideTitle, Math.max(0, timeoutMs), lightbox, lightboxEnabled, renderEntry);
		return;
	}

	showState(root, 'Loading post…', false);
	const request = fetchPostJson(
		window.fetch.bind(window),
		isLatest
			? `${configured.origin}/public/blogs/${encodeURIComponent(blog)}/posts?page=1&size=1`
			: apiUrl(configured.origin, blog, resolution.slug),
		Math.max(0, timeoutMs)
	);
	request
		.then((result) => {
			if (result.error) {
				const { kind, status } = result.error;
				const friendly = kind === 'http' && status === 404;
				showState(root, describeFetchFailure(result.error), !friendly);
				return;
			}
			if (isLatest && !Array.isArray(result.post?.items)) {
				showState(root, 'The content API returned an unexpected response.', true);
				return;
			}
			if (isLatest && result.post.items.length === 0) {
				root.textContent = '';
				return;
			}
			const post = isLatest ? result.post.items[0] : result.post;
			if (
				!post ||
				typeof post.content_markdown !== 'string' ||
				typeof post.title !== 'string'
			) {
				showState(root, 'The content API returned an unexpected response.', true);
				return;
			}
			render(post);
		});
}

/**
 * One rendered entry per post, each with its own metadata map (the
 * payload ships per-post `media` fields).
 */
function renderEntry(document, parent, post, hideTitle, lightbox, enabled) {
	const mediaByUrl = mediaByUrlOf(document.defaultView, post);
	const render = buildMediaRenderer(document.defaultView, mediaByUrl) ?? null;
	const entry = element(document, 'article', 'statesu-announcement');
	renderPost(document.defaultView, entry, post, render, hideTitle);
	attachMediaMetadata(entry, mediaByUrl);
	annotateLinkedImages(entry);
	attachLightboxStage(entry, lightbox, enabled);
	parent.appendChild(entry);
}

async function initFeed(window, root, origin, blog, hideTitle, timeoutMs, lightbox, lightboxEnabled, renderEntry) {
	const document = window.document;
	const posts = element(document, 'div', 'statesu-feed');
	const status = element(document, 'div');
	root.append(posts, status);
	const size = 20;

	async function load(page) {
		showState(status, 'Loading announcements…', false);
		const url = `${origin}/public/blogs/${encodeURIComponent(blog)}/posts?page=${page}&size=${size}`;
		const result = await fetchPostJson(window.fetch.bind(window), url, timeoutMs);
		if (result.error) {
			showState(status, 'Announcements could not be loaded.', true);
			const retry = element(document, 'button', 'statesu-more', 'Tekrar dene');
			retry.type = 'button';
			retry.addEventListener('click', () => void load(page));
			status.appendChild(retry);
			return;
		}
		const data = result.post;
		if (!data || !Array.isArray(data.items) || !Number.isInteger(data.total) || data.total < 0 ||
			!data.items.every((post) => post && typeof post.title === 'string' && typeof post.content_markdown === 'string')) {
			showState(status, 'The content API returned an unexpected response.', true);
			return;
		}
		status.textContent = '';
		for (const post of data.items) {
			renderEntry(document, posts, post, hideTitle, lightbox, lightboxEnabled);
		}
		if (page === 1 && data.items.length === 0) {
			showState(status, 'Henüz duyuru yok.', false);
		} else if (data.items.length > 0 && page * size < data.total) {
			const more = element(document, 'button', 'statesu-more', 'Daha fazla');
			more.type = 'button';
			more.addEventListener('click', () => void load(page + 1));
			status.appendChild(more);
		}
	}

	await load(1);
}

if (typeof window !== 'undefined' && typeof window.document !== 'undefined') {
	init(window);
}
