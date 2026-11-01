<svelte:head>
	<title>Statesu — a small headless Markdown CMS</title>
	<meta name="description" content="Write Markdown in your terminal or dashboard. Publish posts on your own website with Statesu." />
	<link rel="canonical" href="https://statesu.com/" />
	<meta property="og:title" content="Statesu — a small headless Markdown CMS" />
	<meta property="og:description" content="Write Markdown. Publish to your own website." />
	<meta property="og:type" content="website" />
	<meta property="og:url" content="https://statesu.com/" />
</svelte:head>

<header class="header">
	<a class="brand" href="/">Statesu</a>
	<nav aria-label="Main navigation">
		<a href="/docs/cli">CLI</a>
		<a href="/docs/api">API</a>
		<a href="/docs/integration">Web integration</a>
		<a href="/dashboard">Dashboard</a>
		<a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a>
	</nav>
</header>

<main>
	<section class="intro">
		<p class="eyebrow">A small headless Markdown CMS.</p>
		<h1>Your words.<br />Your website.</h1>
		<p class="lead">Write Markdown in your terminal or dashboard. Publish to your own website — without a Git workflow or a new frontend.</p>
		<p><a href="/dashboard">Open dashboard →</a></p>
	</section>

	<section class="guides" aria-label="Choose your guide">
		<a class="guide" href="/docs/cli">
			<h2>Write with the CLI <span aria-hidden="true">↗</span></h2>
			<p>Install the CLI, connect your account, and write or publish from your editor.</p>
		</a>
		<a class="guide" href="/docs/integration">
			<h2>Connect your website <span aria-hidden="true">↗</span></h2>
			<p>Display posts with an embed script, or fetch JSON for your own renderer.</p>
		</a>
		<a class="guide" href="/docs/api">
			<h2>Build with the API <span aria-hidden="true">↗</span></h2>
			<p>Use tokens and HTTP endpoints for custom tools and automation.</p>
		</a>
	</section>
</main>

<footer>
	<span>Statesu — your content, your frontend.</span>
	<a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a>
</footer>

<style>
	.guides { display: grid; gap: 32px; }
	.guide { display: block; text-decoration: none; }
	.guide h2 { font-size: 22px; margin-bottom: 8px; }
	.guide p { margin: 0; }
	.guide:hover h2 { text-decoration: underline; text-underline-offset: 5px; }
	.guide span { font-size: 16px; }
</style>
