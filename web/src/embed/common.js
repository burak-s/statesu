// Shared embed plumbing: both v1 and v2 use the same fetch deadline, DOM
// element helpers and operating constants. Nothing here touches rendering
// or storage; everything is testable without a network.

import { SLUG_PATTERN, MAX_SLUG_LENGTH } from './url.js';

export const DEFAULT_TARGET_ID = 'statesu-post';
export const FETCH_TIMEOUT_MS = 12_000;

export function validBlogSlug(blog) {
	return typeof blog === 'string' && blog.length <= MAX_SLUG_LENGTH && SLUG_PATTERN.test(blog);
}

export function element(document, tag, className, text) {
	const node = document.createElement(tag);
	if (className) node.className = className;
	if (text !== undefined) node.textContent = text;
	return node;
}

/// States render inside the retained root so the host element keeps one
/// stable child for its whole life. `root` must stay attached to the page.
export function showState(root, message, isError) {
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
export async function fetchPostJson(fetch, api, signalTimeoutMs) {
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

/// One stable table of fetch failures shared by both embed versions, so
/// the wording never drifts between v1 and v2.
export function describeFetchFailure(error) {
	const { kind, status } = error;
	if (kind === 'timeout') return 'The content API did not answer in time. Try again.';
	if (kind === 'http' && status === 404) return 'No published post at this address.';
	if (kind === 'http') return `The content API returned ${status}.`;
	if (kind === 'bad-response') return 'The content API returned an unparsable response.';
	return 'The content API could not be reached. If this runs in a browser, the site origin must be allowed for this blog.';
}

/// Shared body rendering: title heading plus sanitized content, either
/// through a renderer or as literal text when no renderer could exist.
export function renderPost(window, parent, post, render, hideTitle) {
	const document = window.document;
	if (!hideTitle) {
		const heading = element(document, 'h2', 'statesu-title', post.title);
		parent.appendChild(heading);
	}
	const body = element(document, render === null ? 'pre' : 'div',
		render === null ? 'statesu-embed-fallback statesu-body' : 'statesu-body');
	if (render === null) {
		body.textContent = post.content_markdown;
	} else {
		body.innerHTML = render(post.content_markdown);
	}
	parent.appendChild(body);
}
