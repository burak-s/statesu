<svelte:head>
	<title>Statesu — a blog for your existing website</title>
	<meta name="description" content="A blog for your website, without rebuilding your website. Write in Markdown. Publish through an embed or API. Keep your existing site." />
	<link rel="canonical" href="https://statesu.com/" />
	<meta property="og:title" content="Statesu — a blog for your existing website" />
	<meta property="og:description" content="Write in Markdown. Publish through an embed or API. Keep your existing site." />
	<meta property="og:type" content="website" />
	<meta property="og:url" content="https://statesu.com/" />
	<meta name="twitter:card" content="summary" />
	<meta name="twitter:title" content="Statesu — a blog for your existing website" />
	<meta name="twitter:description" content="Write in Markdown. Publish through an embed or API. Keep your existing site." />
	<script type="application/ld+json">
		{
			"@context": "https://schema.org",
			"@type": "SoftwareApplication",
			"name": "Statesu",
			"url": "https://statesu.com/",
			"description": "A blog for your website, without rebuilding your website. Write in Markdown. Publish through an embed or API. Keep your existing site.",
			"applicationCategory": "DeveloperApplication",
			"operatingSystem": "Web",
			"sameAs": "https://github.com/burak-s/statesu"
		}
	</script>
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
		<p class="eyebrow">Your words. Your website.</p>
		<h1>A blog for your website,<br />without rebuilding your website.</h1>
		<p class="lead">Write in Markdown. Publish through an embed or API. Keep your existing site.</p>
		<p><a href="/dashboard">Open dashboard →</a></p>
	</section>

	<section class="guides" aria-label="Choose your guide">
		<a class="guide" href="/docs/cli">
			<h2>Write with the CLI <span aria-hidden="true">↗</span></h2>
			<p>Write in your favorite editor and publish from your terminal. Prefer the browser? Use the dashboard.</p>
		</a>
		<a class="guide" href="/docs/integration">
			<h2>Connect your website <span aria-hidden="true">↗</span></h2>
			<p>Add an embed script to display your posts. Keep your website, its design, and your existing setup.</p>
		</a>
		<a class="guide" href="/docs/api">
			<h2>Build with the API <span aria-hidden="true">↗</span></h2>
			<p>Fetch posts for your own frontend, or connect your publishing tools and automation.</p>
		</a>
	</section>
</main>

<footer>
	<span>Statesu — your blog, your website.</span>
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
