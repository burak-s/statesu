// Thin fetch client for the Statesu API. Bearer-only: the token lives in
// localStorage and every request sends `Authorization: Bearer`. Responses use
// `{"error": "..."}` bodies, which surface directly in the UI.

const TOKEN_KEY = 'statesu_token';

export function loadToken() {
	if (typeof localStorage === 'undefined') return null;
	return localStorage.getItem(TOKEN_KEY);
}

export function storeToken(token) {
	if (token === null) return localStorage.removeItem(TOKEN_KEY);
	localStorage.setItem(TOKEN_KEY, token);
}

export function isEmail(input) {
	return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(input);
}

export async function request(path, { method = 'GET', token, body } = {}) {
	let response;
	try {
		const headers = {};
		if (token) headers['Authorization'] = `Bearer ${token}`;
		if (body !== undefined) headers['Content-Type'] = 'application/json';
		response = await fetch(path, {
			method,
			headers,
			body: body === undefined ? undefined : JSON.stringify(body)
		});
	} catch {
		return { ok: false, data: null, error: 'network error — is the API running?' };
	}

	let data = null;
	try {
		data = await response.json();
	} catch {
		// 204-style responses have no body; that is fine.
	}
	if (!response.ok) {
		return { ok: false, data, error: data?.error ?? `request failed with ${response.status}` };
	}
	return { ok: true, data, error: null };
}
