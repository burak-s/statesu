<script>
	import { request } from '#lib/api.js';

	let stage = $state('working'); // working | verified | failed
	let detail = $state('');

	$effect(() => {
		if (stage !== 'working') return;
		const token = new URLSearchParams(window.location.search).get('token') ?? '';
		if (!token) {
			stage = 'failed';
			detail = 'This link does not carry a confirmation token.';
			return;
		}
		confirm(token);
	});

	/** @param {string} token */
	async function confirm(token) {
		const result = await request('/auth/verify', { method: 'POST', body: { token } });
		if (result.ok) {
			stage = 'verified';
		} else {
			stage = 'failed';
			// American-style plain messages for the stable API error strings.
			detail =
				result.error === 'token expired'
					? 'The confirmation link has expired. Request a fresh mail from the dashboard.'
					: result.error === 'invalid token'
						? 'The confirmation link is invalid or was already used. If your mail is older than 30 minutes, request a fresh one.'
						: result.error;
		}
	}
</script>

<svelte:head>
	<title>Statesu — verify email</title>
	<meta name="robots" content="noindex" />
</svelte:head>

<section class="narrow">
	{#if stage === 'working'}
		<h1>Verifying…</h1>
		<p role="status">Checking the confirmation link.</p>
	{:else if stage === 'verified'}
		<h1>Email verified</h1>
		<p>Your address is confirmed. <a href="/dashboard">Sign in</a> to start publishing.</p>
	{:else}
		<h1>Verification failed</h1>
		<p class="error" role="alert">{detail}</p>
		<p><a href="/dashboard">Back to the dashboard</a> to sign in or request a new mail.</p>
	{/if}
</section>

<style>
	.narrow { padding-block: 40px 200px; max-width: 640px; margin: 0 auto; }
	.error { color: #171717; background: #f2f2f2; padding: 10px 12px; border-left: 3px solid #171717; }
</style>
