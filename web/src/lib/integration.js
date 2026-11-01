// Share validation rules for the dashboard's website integration panel.
// Extracted into a pure module so the rules the dashboard shows the user are
// also unit-testable outside Svelte.

/**
 * Site address to origin: accepts `example.com` or a full origin.
 * Returns the `scheme://host[:port]` origin, or '' when unusable.
 */
export function originFromSiteAddress(input) {
	const raw = String(input ?? '').trim();
	if (!raw) return '';
	const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(raw) ? raw : `https://${raw}`;
	try {
		const url = new URL(withScheme);
		if (url.protocol !== 'http:' && url.protocol !== 'https:') return '';
		return url.origin;
	} catch {
		return '';
	}
}

/// Posts path: `/blog`-style single segment, or exactly `/`.
export function isValidPostPath(path) {
	const value = String(path ?? '').trim();
	return value === '/' || /^\/[a-z0-9-]+$/.test(value);
}
