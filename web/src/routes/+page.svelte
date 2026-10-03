<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const title = 'Statesu — Dynamic User Status API';
	const description = 'An HTTP API for dynamic user statuses. Update your status as it changes, retrieve it as JSON, and display it in your own apps, websites, and integrations.';
	const canonicalUrl = 'https://statesu.com/';
	const structuredData = {
		'@context': 'https://schema.org',
		'@type': 'TechArticle',
		'@id': `${canonicalUrl}#documentation`,
		headline: title,
		description,
		url: canonicalUrl,
		inLanguage: 'en',
		about: {
			'@type': 'WebAPI',
			name: 'Statesu',
			url: canonicalUrl,
			description: 'An HTTP API for dynamic user statuses, with public reads for custom clients, apps, websites, and integrations.'
		}
	};

	const examples = {
		setup: `API=https://statesu.com
# For a local server: API=http://localhost:8080
EMAIL=you@example.com`,
		register: `# Bash: prompt without saving your password in shell history.
read -r -s -p 'Password: ' PASSWORD; printf '\\n'
TOKEN=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \\
  '{email: $email, password: $password}' | \\
  curl --fail-with-body -sS "$API/auth/register" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.token')
unset PASSWORD`,
		login: `read -r -s -p 'Password: ' PASSWORD; printf '\\n'
TOKEN=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \\
  '{email: $email, password: $password}' | \\
  curl --fail-with-body -sS "$API/auth/login" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.token')
unset PASSWORD`,
		post: `# Unix seconds + 1 day. Works with GNU and BSD date.
EXPIRES_AT=$(( $(date +%s) + 86400 ))
STATE_ID=$(jq -n --arg text 'shipping something small' \\
  --argjson expires_at "$EXPIRES_AT" \\
  '{text: $text, expires_at: $expires_at}' | \\
  curl --fail-with-body -sS "$API/state" \\
    -H "Authorization: Bearer $TOKEN" \\
    -H 'Content-Type: application/json' --data-binary @- | \\
  jq -er '.state_id')`,
		latest: `curl --fail-with-body -sS -G "$API/state/latest" \\
  --data-urlencode "email=$EMAIL" | jq .

# Only the text — useful in scripts, status bars and dotfiles.
curl --fail-with-body -sS -G "$API/state/latest" \\
  --data-urlencode "email=$EMAIL" | jq -r '.text'

# Omit email to read the latest state from anyone.
curl --fail-with-body -sS "$API/state/latest" | jq .`,
		list: `curl --fail-with-body -sS -G "$API/state" \\
  --data-urlencode "email=$EMAIL" \\
  --data-urlencode 'page=1' --data-urlencode 'size=20' | jq .`,
		remove: `curl --fail-with-body -sS -X DELETE "$API/state/$STATE_ID" \\
  -H "Authorization: Bearer $TOKEN"
# Success: HTTP 204, no response body.`,
		response: `{
  "state_id": "example-state-id",
  "user_id": "example-user-id",
  "email": "you@example.com",
  "text": "shipping something small",
  "created_at": 1760000000,
  "expires_at": 1760086400
}`,
		error: `{"error":"token expired"}`
	};
	const wgetAuth = (path) => `# Bash: prompt without saving your password in shell history.
read -r -s -p 'Password: ' PASSWORD; printf '\\n'
BODY=$(jq -n --arg email "$EMAIL" --arg password "$PASSWORD" \\
  '{email: $email, password: $password}')
TOKEN=$(wget -O- --header='Content-Type: application/json' \\
  --post-data="$BODY" "$API${path}" | jq -er '.token')
unset PASSWORD BODY`;
	const wgetExamples = {
		register: wgetAuth('/auth/register'),
		login: wgetAuth('/auth/login'),
		post: `# Unix seconds + 1 day. Works with GNU and BSD date.
EXPIRES_AT=$(( $(date +%s) + 86400 ))
BODY=$(jq -n --arg text 'shipping something small' \\
  --argjson expires_at "$EXPIRES_AT" \\
  '{text: $text, expires_at: $expires_at}')
STATE_ID=$(wget -O- --header="Authorization: Bearer $TOKEN" \\
  --header='Content-Type: application/json' \\
  --post-data="$BODY" "$API/state" | jq -er '.state_id')`,
		latest: `# Encode the email before adding it to the URL.
ENCODED_EMAIL=$(printf '%s' "$EMAIL" | jq -sRr @uri)
wget -O- "$API/state/latest?email=$ENCODED_EMAIL" | jq .

# Only the text — useful in scripts, status bars and dotfiles.
wget -O- "$API/state/latest?email=$ENCODED_EMAIL" | jq -r '.text'

# Omit email to read the latest state from anyone.
wget -O- "$API/state/latest" | jq .`,
		list: `ENCODED_EMAIL=$(printf '%s' "$EMAIL" | jq -sRr @uri)
wget -O- "$API/state?email=$ENCODED_EMAIL&page=1&size=20" | jq .`,
		remove: `wget -O- --method=DELETE "$API/state/$STATE_ID" \\
  --header="Authorization: Bearer $TOKEN"
# Success: HTTP 204, no response body.`
	};
	const endpoints = [
		['POST', '/auth/register', 'No', 'Create an account; returns id, email, token and expires_at. 201.'],
		['POST', '/auth/login', 'No', 'Authenticate with email and password; returns the same token fields. 200.'],
		['POST', '/state', 'Bearer', 'Body: text and expires_at. Returns the new state. 201.'],
		['GET', '/state?email=…', 'No', 'Required email; optional page and size. Returns items, page, size and total. 200.'],
		['GET', '/state/latest', 'No', 'Optional email filter. Returns a state with its owner’s email. 200.'],
		['DELETE', '/state/{stateID}', 'Bearer', 'Delete your own state. No response body. 204.'],
		['GET', '/auth/me', 'Bearer', 'Check your token; returns id and email. 200.']
	];
</script>

<svelte:head>
	<title>{title}</title>
	<meta name="description" content={description} />
	<link rel="canonical" href={canonicalUrl} />
	<meta property="og:type" content="website" />
	<meta property="og:site_name" content="Statesu" />
	<meta property="og:title" content={title} />
	<meta property="og:description" content={description} />
	<meta property="og:url" content={canonicalUrl} />
	<meta property="og:locale" content="en_US" />
	<meta name="twitter:card" content="summary" />
	<meta name="twitter:title" content={title} />
	<meta name="twitter:description" content={description} />
	{@html `<script type="application/ld+json">${JSON.stringify(structuredData)}</script>`}
</svelte:head>

<header class="header">
	<a class="brand" href="/">Statesu</a>
	<nav aria-label="Documentation">
		<a href="#quickstart">Quick start</a>
		<a href="#manage">Manage states</a>
		<a href="#reference">API reference</a>
	</nav>
</header>

<main>
	<section class="intro">
		<p class="eyebrow">Dynamic user statuses over HTTP.</p>
		<h1>Your status.<br />Wherever you want it.</h1>
		<p class="lead">An API for dynamic user statuses. Update your status as it changes and retrieve it wherever you want to display it.</p>
		<p>Call the API from your terminal, write your own client, or integrate it into an app. Statesu stores your updates and returns them as JSON; you decide how to publish and display them.</p>
		<p>Each status update is called a state. Fetch a user's latest state to show their current status on a website, in an app, or in any other interface you build.</p>
	</section>

	<section id="quickstart">
		<h2>Get started with the API</h2>
		<p>The examples below use curl or wget, but the same endpoints work with any HTTP client. For these examples, you'll need Bash, <a href="https://jqlang.org/">jq</a>, and either curl 7.76+ or GNU wget 1.15+. Select your HTTP client in each example; jq builds and reads the JSON. Run all commands in the same shell so your variables stay available, and replace <code>you@example.com</code> with your email address.</p>
		<Terminal code={examples.setup} />

		<h3>01 / Create an account</h3>
		<p>Register once. Use a password of 8–72 bytes with at least one letter and one digit. The response includes a bearer token; the command below stores it in <code>TOKEN</code>.</p>
		<Terminal variants={{ curl: examples.register, wget: wgetExamples.register }} />
		<p>If a command fails, stop and resolve the error before continuing. Treat the token like a password: don't commit it, paste it into public logs, or share it.</p>

		<h3>02 / Publish a state</h3>
		<p>Send your status text and an expiration timestamp. This example sets the expiration to one day from now and saves the state ID for deletion later. Expiration does not automatically hide or delete the state.</p>
		<Terminal variants={{ curl: examples.post, wget: wgetExamples.post }} />
		<p>Text is trimmed, must not be empty, and has a 4096-byte limit. <code>expires_at</code> is required: Unix seconds, in the future, at most 30 days away. It is not a duration or a millisecond timestamp.</p>

		<h3>03 / Display it anywhere</h3>
		<p>Fetch your latest state by email and use the <code>text</code> field wherever you want to show your status. Your client controls the display and when to fetch updates. Reading is public and needs no token. Always URL-encode email query parameters, especially addresses containing <code>+</code>.</p>
		<Terminal variants={{ curl: examples.latest, wget: wgetExamples.latest }} />
		<p>The latest-state endpoint returns JSON like this. The IDs and timestamps below are examples.</p>
		<Terminal code={examples.response} language="json" />
		<p><strong>Public by design:</strong> anyone can read states, and the latest-state response includes the owner's email. Don't post secrets. Expiration is metadata: the current read endpoints still return expired states. Check <code>expires_at</code> in your client if you only want active states.</p>
	</section>

	<section id="manage">
		<h2>Manage your status updates</h2>
		<h3>Get a fresh token</h3>
		<p>Already registered? Use login instead. Authentication responses include the token's expiration time in <code>expires_at</code>. When a token expires, log in again.</p>
		<Terminal variants={{ curl: examples.login, wget: wgetExamples.login }} />
		<h3>List your states</h3>
		<p>States are returned newest first. Page numbering starts at 1; the default size is 20 and the maximum is 100. An unknown email returns an empty list.</p>
		<Terminal variants={{ curl: examples.list, wget: wgetExamples.list }} />
		<h3>Delete a state</h3>
		<p>Use the <code>STATE_ID</code> saved when publishing, or a <code>state_id</code> from your history. Only the owner can delete a state.</p>
		<Terminal variants={{ curl: examples.remove, wget: wgetExamples.remove }} />
	</section>


	<section id="reference">
		<h2>API reference</h2>
		<p>Base URL: <code>https://statesu.com</code>. Request and response bodies are JSON. Send <code>Content-Type: application/json</code> for JSON bodies and <code>Authorization: Bearer $TOKEN</code> for authenticated requests.</p>
		<div class="table-scroll">
			<table>
				<thead><tr><th>Method / endpoint</th><th>Auth</th><th>Behavior</th></tr></thead>
				<tbody>
					{#each endpoints as [method, path, auth, description]}
						<tr><td><code>{method} {path}</code></td><td>{auth}</td><td>{description}</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
		<h3>Error responses and HTTP status codes</h3>
		<Terminal code={examples.error} language="json" />
		<ul>
			<li><strong>400</strong> — invalid JSON, invalid credentials format, missing email, or invalid text / expiration.</li>
			<li><strong>401</strong> — incorrect login credentials, or a missing, invalid or expired token.</li>
			<li><strong>404</strong> — no latest state, or the state to delete doesn't exist / isn't yours.</li>
			<li><strong>409</strong> — an account with that email already exists; log in instead.</li>
			<li><strong>500</strong> — server error; retry later.</li>
		</ul>
		<p><code>curl --fail-with-body</code> preserves the error body and exits nonzero on HTTP errors. Use curl's <code>-i</code> or wget's <code>--server-response</code> to inspect HTTP status and headers. wget also exits nonzero on HTTP errors. Clear your shell token when finished: <code>unset TOKEN</code>.</p>
	</section>
</main>

<footer>Statesu — dynamic user status API. <a href="#quickstart">Back to quick start ↑</a></footer>
