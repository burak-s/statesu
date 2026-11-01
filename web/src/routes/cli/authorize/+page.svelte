<script>
	import { onMount } from 'svelte';
	import { request } from '#lib/api.js';

	let loading = $state(true);
	let email = $state('');
	let error = $state('');
	let busy = $state(false);
	let signInUrl = $state('/dashboard');
	/** @type {{ redirect_uri: string, code_challenge: string, state: string } | null} */
	let authorization = $state(null);

	onMount(async () => {
		const params = new URLSearchParams(window.location.search);
		const redirect_uri = params.get('redirect_uri') ?? '';
		const code_challenge = params.get('code_challenge') ?? '';
		const state = params.get('state') ?? '';
		try {
			const callback = new URL(redirect_uri);
			if (callback.protocol !== 'http:' || callback.hostname !== '127.0.0.1' ||
				!callback.port || callback.pathname !== '/callback' || callback.search ||
				callback.hash || callback.username || callback.password ||
				!/^[A-Za-z0-9_-]{43}$/.test(code_challenge) || !/^[a-f0-9]{64}$/.test(state)) {
				throw new Error('Invalid CLI authorization link. Start again with stscli login.');
			}
			authorization = { redirect_uri, code_challenge, state };
			signInUrl = '/dashboard?next=' + encodeURIComponent('/cli/authorize' + window.location.search);
			const result = await request('/auth/me');
			if (result.ok) email = result.data.email;
			else if (!result.unauthorized) error = result.error;
		} catch (cause) {
			error = cause instanceof Error ? cause.message : 'Invalid authorization link.';
		} finally {
			loading = false;
		}
	});

	async function approve() {
		if (!authorization || busy) return;
		busy = true;
		error = '';
		const result = await request('/auth/cli/authorize', { method: 'POST', body: authorization });
		if (result.ok) {
			// Recheck the server's redirect against the exact requested callback.
			const redirect = new URL(result.data.redirect);
			const expected = new URL(authorization.redirect_uri);
			if (redirect.origin === expected.origin && redirect.pathname === expected.pathname &&
				redirect.searchParams.get('state') === authorization.state) {
				window.location.replace(redirect.href);
				return;
			}
			error = 'Unexpected callback address. Restart login from your terminal.';
		} else {
			error = result.error;
			if (result.unauthorized) email = '';
		}
		busy = false;
	}
</script>

<svelte:head>
	<title>Authorize CLI — Statesu</title>
	<meta name="robots" content="noindex, nofollow" />
	<meta name="referrer" content="no-referrer" />
</svelte:head>

<section>
	<h1>Connect Statesu CLI</h1>
	{#if loading}
		<p>Checking your session…</p>
	{:else}
		{#if error}<p role="alert">{error}</p>{/if}
		{#if authorization}
			{#if email}
				<p>Signed in as <strong>{email}</strong>.</p>
				<p>Allow Statesu CLI to read and manage your blogs and posts? Access stays active until revoked in the dashboard or with <code>stscli logout</code>.</p>
				<p>Only approve if you just ran <code>stscli login</code> on this computer. Do not approve links sent by someone else.</p>
				<button onclick={approve} disabled={busy}>{busy ? 'Connecting…' : 'Authorize CLI'}</button>
				<a href="/dashboard">Cancel</a>
			{:else}
				<p>Sign in on the website first, then return here to approve CLI access.</p>
				<a href={signInUrl}>Sign in to continue</a>
			{/if}
		{/if}
	{/if}
</section>
