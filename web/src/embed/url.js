// Pure URL resolution for the Statesu embed: which post, if any, does the
// current page address stand for? Rules mirror the backend slug validation
// (lowercase letters, digits, hyphens, at most 63 bytes).

export const MAX_SLUG_LENGTH = 63;
export const SLUG_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Normalizes a `data-base-path` value to a site-absolute path.
 * Returns `/blog`-style strings or `/`; throws on anything else.
 */
export function parseBasePath(input) {
	if (input === undefined || input === null || input === '') return '/blog';
	if (typeof input !== 'string') throw new Error('basePath must be a string');
	if (input === '/') return '/';
	if (!/^\//.test(input)) throw new Error('basePath must start with /');
	const noTrailing = input.replace(/\/+$/, '');
	if (noTrailing === '') return '/';
	if (!/^\/[a-z0-9-]+$/.test(noTrailing)) {
		throw new Error('basePath must be a single lowercase segment, or /');
	}
	return noTrailing;
}

function decoded(value) {
	try {
		return { ok: true, value: decodeURIComponent(value) };
	} catch {
		return { ok: false, value: null };
	}
}

/** A post slug must match backend rules, one decode round, no encodings. */
function toPost(slug) {
	const raw = decoded(slug);
	if (!raw.ok || raw.value !== slug || !SLUG_PATTERN.test(raw.value) || raw.value.length > MAX_SLUG_LENGTH) {
		return { kind: 'invalid', reason: 'slug' };
	}
	return { kind: 'post', slug: raw.value };
}

/**
 * Resolves the page URL against the embed's base path.
 * Returns one of:
 *   { kind: 'post', slug }   — render this post
 *   { kind: 'empty' }        — the base path itself; not a list
 *   { kind: 'skip' }         — outside the base path; do not touch the page
 *   { kind: 'invalid' }      — inside the base path but not a valid post URL
 */
export function resolveSlug(siteUrl, basePath) {
	const url = new URL(siteUrl);
	if (basePath === '/') {
		if (url.pathname === '/' || url.pathname === '') return { kind: 'empty' };
		let rest = url.pathname.slice(1);
		if (rest.endsWith('/')) rest = rest.slice(0, -1);
		if (rest === '' || rest.includes('/')) return { kind: 'invalid', reason: 'multi-segment' };
		return toPost(rest);
	}
	const prefix = `${basePath}/`;
	if (url.pathname === basePath) return { kind: 'empty' };
	if (!url.pathname.startsWith(prefix)) return { kind: 'skip' };
	const rest = url.pathname.slice(prefix.length);
	if (rest.startsWith('/')) return { kind: 'invalid', reason: 'multi-segment' };
	// One visible trailing slash is tolerated; query and hash never reach the
	// slug because `URL.pathname` excludes them. Encoded slashes stay encoded
	// and fail the slug pattern, so no double-decoding laundering happens.
	const trimmed = rest.endsWith('/') ? rest.slice(0, -1) : rest;
	if (trimmed === '') return { kind: 'empty' };
	if (trimmed.includes('/')) return { kind: 'invalid', reason: 'multi-segment' };
	return toPost(trimmed);
}
