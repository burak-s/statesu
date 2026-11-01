// Builds the public delivery API request URL for the embed. The API origin
// comes from the embed script's own address unless an explicit override is
// configured; blog and post slugs are percent-encoded.
// Production runs over HTTPS; plain HTTP is accepted only for local
// development hosts so an accidentally HTTP-embedded script cannot silently
// downgrade a live site's API traffic.

const LOCAL_ONLY_HOSTS = new Set(['localhost', '127.0.0.1', '0.0.0.0', '[::1]']);

/**
 * Resolves the configured script address to an API origin.
 * Returns `{ ok: true, origin }` or `{ ok: false, reason }`.
 */
export function parseApiOrigin(src) {
	let url;
	try {
		url = new URL(src);
	} catch {
		return { ok: false, reason: 'not-a-url' };
	}
	if (url.protocol === 'https:') return { ok: true, origin: url.origin };
	if (url.protocol === 'http:' && LOCAL_ONLY_HOSTS.has(url.hostname)) {
		return { ok: true, origin: url.origin };
	}
	return { ok: false, reason: 'insecure' };
}

export function apiUrl(origin, blogSlug, postSlug) {
	return `${origin}/public/blogs/${encodeURIComponent(blogSlug)}/posts/${encodeURIComponent(postSlug)}`;
}
