// Thin fetch client for the Statesu API. Browser sessions ride HttpOnly
// cookies; no token is ever stored in JavaScript persistence. Unsafe requests
// echo the readable CSRF cookie back as `X-CSRF-Token`. Responses use
// `{"error": "..."}` bodies, which surface directly in the UI.

const CSRF_COOKIE = '__Host-statesu_csrf';
const CSRF_HEADER = 'x-csrf-token';

/** @param {string} input */
export function isEmail(input) {
	return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(input);
}

function readCsrfToken() {
	if (typeof document === 'undefined') return '';
	for (const pair of document.cookie.split(';')) {
		const [name, ...rest] = pair.trim().split('=');
		if (name === CSRF_COOKIE) return rest.join('=');
	}
	return '';
}

export const UNSAFE_METHODS = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

/**
 * @typedef {{ ok: true, data: any | null, error: null, unauthorized: false }} OkResult
 * @typedef {{ ok: false, data: any | null, error: string, unauthorized: boolean }} ErrResult
 * @typedef {OkResult | ErrResult} Result
 */

/**
 * @param {string} path
 * @param {{ method?: string, body?: unknown }} [options]
 * @returns {Promise<Result>}
 */
export async function request(path, { method = 'GET', body } = {}) {
	const unsafe = UNSAFE_METHODS.has(method.toUpperCase());
	let response;
	try {
		/** @type {{ [key: string]: string }} */
		const headers = {};
		if (unsafe) headers[CSRF_HEADER] = readCsrfToken();
		if (body !== undefined) headers['Content-Type'] = 'application/json';
		response = await fetch(path, {
			method,
			headers,
			credentials: 'same-origin',
			body: body === undefined ? undefined : JSON.stringify(body)
		});
	} catch {
		return { ok: false, data: null, error: 'network error — is the API running?', unauthorized: false };
	}

	let data = null;
	try {
		data = await response.json();
	} catch {
		// 204-style responses have no body; that is fine.
	}
	if (response.status === 401) {
		return { ok: false, data, error: data?.error ?? 'unauthorized', unauthorized: true };
	}
	if (!response.ok) {
		return { ok: false, data, error: data?.error ?? `request failed with ${response.status}`, unauthorized: false };
	}
	return { ok: true, data, error: null, unauthorized: false };
}
