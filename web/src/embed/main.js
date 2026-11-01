// Statesu embed v1: renders the published post whose slug the current page
// address contains, exactly once per page, into one target element.
//
//   <script defer
//     src="https://statesu.example/embed/v1.js"
//     data-blog="burak"
//     data-base-path="/blog"
//     data-target="statesu-post"></script>
//
// The base path itself shows a friendly empty state; pages outside it are
// ignored; invalid post URLs fail without touching the API. Only the target
// element's content is changed. The same deadline covers headers and body.

import { parseBasePath, resolveSlug, SLUG_PATTERN, MAX_SLUG_LENGTH } from './url.js';
import { parseApiOrigin, apiUrl } from './api.js';
import { buildRenderer } from './content.js';

const DEFAULT_TARGET_ID = 'statesu-post';
const FETCH_TIMEOUT_MS = 12_000;
const LIBRARY_VERSION = 'v1';

export function validBlogSlug(blog) {
	return typeof blog === 'string' && blog.length <= MAX_SLUG_LENGTH && SLUG_PATTERN.test(blog);
}

function stylesFor(prefix) {
	return `.${prefix} { color: inherit; font-family: inherit; }
.${prefix} .statesu-state { color: inherit; opacity: .75; font-style: italic; }
.${prefix} .statesu-error { color: inherit; opacity: .85; }
.${prefix} .statesu-title { margin: 0 0 .5em; }
.${prefix} .statesu-body { line-height: 1.6; }
.${prefix} .statesu-body pre { overflow-x: auto; }
.${prefix} .statesu-body a { text-decoration: underline; }`;
}

function injectStyles(document) {
	const style = document.createElement('style');
	style.textContent = stylesFor('statesu-embed');
	style.dataset.statesu = LIBRARY_VERSION;
	document.head.appendChild(style);
}

function element(document, tag, className, text) {
	const node = document.createElement(tag);
	if (className) node.className = className;
	if (text !== undefined) node.textContent = text;
	return node;
}

/// States render inside the retained root so the host element keeps one
/// stable child for its whole life. `root` must stay attached to the page.
function showState(root, message, isError) {
	root.textContent = '';
	const node = element(root.ownerDocument, 'p', isError ? 'statesu-error' : 'statesu-state', message);
	node.setAttribute('role', isError ? 'alert' : 'status');
	if (!isError) node.setAttribute('aria-live', 'polite');
	root.appendChild(node);
}

/**
 * Fetches and fully parses a post under one deadline that covers headers and
 * body reads. When the deadline fires the request is actually cancelled:
 * the controller is aborted and the signal rides on the fetch, so a slow or
 * stuck host cannot keep downloading in the background.
 * Returns `{ post }` on success or `{ error: { kind, status? } }`.
 */
async function fetchPostJson(fetch, api, signalTimeoutMs) {
	const controller = new AbortController();
	let timedOut = false;
	let timer;
	const deadline = new Promise((_, reject) => {
		timer = setTimeout(() => {
			timedOut = true;
			controller.abort();
			reject(new DOMException('timeout', 'AbortError'));
		}, signalTimeoutMs);
	});
	try {
		const response = await Promise.race([
			fetch(api, {
				method: 'GET',
				credentials: 'omit',
				headers: { Accept: 'application/json' },
				redirect: 'error',
				signal: controller.signal
			}),
			deadline
		]);
		if (response.status === 0 || response.type === 'opaqueredirect') {
			return { error: { kind: 'network' } };
		}
		if (!response.ok) {
			return { error: { kind: 'http', status: response.status } };
		}
		const post = await Promise.race([response.json(), deadline]);
		return { post };
	} catch (error) {
		if (timedOut || error?.name === 'AbortError') {
			return { error: { kind: 'timeout' } };
		}
		const kind = error?.name === 'SyntaxError' ? 'bad-response' : 'network';
		return { error: { kind } };
	} finally {
		clearTimeout(timer);
	}
}

/**
 * Resolves the embed's own script element: prefer the currently executing
 * script, fall back to the first configured match, and never pick an
 * unrelated host page script that happens to carry a data-blog attribute.
 */
function ownScript(document) {
	const current = document.currentScript;
	if (current && current.getAttribute('data-blog') !== null) return current;
	for (const script of document.querySelectorAll('script[data-blog]')) {
		const src = script.getAttribute('src') ?? '';
		if (src.endsWith('/embed/v1.js') || (current && script === current)) {
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
	if (!document || window.__STATESU_EMBED_V1__) {
		if (window.__STATESU_EMBED_V1__) {
			window.console?.warn?.('Statesu embed already initialized on this page.');
		}
		return;
	}
	window.__STATESU_EMBED_V1__ = true;

	const script = ownScript(document);
	if (!script) return;

	const targetId = script.dataset.target || DEFAULT_TARGET_ID;
	const target = document.getElementById(targetId);
	if (!target) {
		window.console?.error?.(`Statesu embed: target element #${targetId} not found.`);
		return;
	}

	const blog = script.dataset.blog;
	const root = element(document, 'article', 'statesu-embed');
	/// Mounts the retained root; called only when the page needs the embed,
	/// so pages outside the base path stay completely untouched.
	const mount = () => {
		target.textContent = '';
		target.appendChild(root);
		injectStyles(document);
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

	const resolution = resolveSlug(window.location.href, basePath);
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

	showState(root, 'Loading post…', false);
	const request = fetchPostJson(
		window.fetch.bind(window),
		apiUrl(configured.origin, blog, resolution.slug),
		Math.max(0, timeoutMs)
	);
	request
		.then((result) => {
			if (result.error) {
				const { kind, status } = result.error;
				if (kind === 'timeout') {
					showState(root, 'The content API did not answer in time. Try again.', true);
				} else if (kind === 'http' && status === 404) {
					showState(root, 'No published post at this address.', false);
				} else if (kind === 'http') {
					showState(root, `The content API returned ${status}.`, true);
				} else if (kind === 'bad-response') {
					showState(root, 'The content API returned an unparsable response.', true);
				} else {
					showState(
						root,
						'The content API could not be reached. If this runs in a browser, the site origin must be allowed for this blog.',
						true
					);
				}
				return;
			}
			const post = result.post;
			if (
				!post ||
				typeof post.content_markdown !== 'string' ||
				typeof post.title !== 'string'
			) {
				showState(root, 'The content API returned an unexpected response.', true);
				return;
			}
			const render = buildRenderer(window);
			root.textContent = '';
			const heading = element(document, 'h2', 'statesu-title');
			heading.textContent = post.title;
			root.appendChild(heading);
			if (render === null) {
				// Fail closed: no safe pipeline, no HTML. Show the raw Markdown
				// as inert text instead of skipping content.
				const body = element(document, 'pre', 'statesu-embed-fallback statesu-body');
				body.textContent = post.content_markdown;
				root.appendChild(body);
				return;
			}
			const body = element(document, 'div', 'statesu-body');
			body.innerHTML = render(post.content_markdown);
			root.appendChild(body);
		});
}

if (typeof window !== 'undefined' && typeof window.document !== 'undefined') {
	init(window);
}
