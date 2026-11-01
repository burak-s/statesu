<script>
	// Cloudflare Turnstile widget. Loads api.js once per document, renders the
	// challenge and keeps the one-time verification token in `token`; `reset()`
	// must be called after each submission because tokens are single-use.

	/** @type {string} */
	let { sitekey, theme = 'light', token = $bindable('') } = $props();

	const SCRIPT_SRC = 'https://challenges.cloudflare.com/turnstile/v0/api.js';

	/** @type {HTMLElement | null} */
	let host = $state(null);
	/** @type {string | null} */
	let widget = $state(null);

	/** Loads api.js exactly once per document and resolves when the API exists. */
	function ready() {
		if (window.turnstile) return Promise.resolve();
		return new Promise((resolve, reject) => {
			let script = /** @type {HTMLScriptElement | null} */ (
				document.querySelector(`script[src="${SCRIPT_SRC}"]`)
			);
			if (!script) {
				script = document.createElement('script');
				script.src = SCRIPT_SRC;
				script.async = true;
				script.defer = true;
				document.head.appendChild(script);
			}
			script.addEventListener('load', () => resolve(window.turnstile));
			script.addEventListener('error', () => reject(new Error('Cloudflare Turnstile could not load')));
		});
	}

	$effect(() => {
		if (!host || !sitekey) return;
		let disposed = false;
		(async () => {
			try {
				await ready();
			} catch {
				// Script blocked or offline: no widget appears; the server still
				// rejects the credential flow, the UI keeps working.
				return;
			}
			if (disposed || !host) return;
			widget = window.turnstile.render(host, {
				sitekey,
				theme,
				callback: /** @param {string} value */ (value) => (token = value),
				'expired-callback': () => (token = ''),
				'error-callback': () => (token = '')
			});
		})();
		return () => {
			disposed = true;
			if (widget !== null) {
				window.turnstile?.remove(widget);
				widget = null;
			}
		};
	});

	/** Consumes the token challenge again after a submission. */
	export function reset() {
		token = '';
		if (widget !== null) window.turnstile?.reset(widget);
	}
</script>

<div bind:this={host} class="challenge" data-testid="turnstile-widget"></div>
