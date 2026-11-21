<script>
	import Seo from '#lib/components/Seo.svelte';
</script>

<Seo
	title="Add a Markdown Blog to Your Website"
	description="Add a Markdown blog to your existing website without rebuilding it. Publish from your terminal or dashboard, then display posts with an embed or API."
	path="/"
/>

<svelte:head>
	<script type="application/ld+json">
		{
			"@context": "https://schema.org",
			"@graph": [
				{
					"@type": "WebSite",
					"@id": "https://statesu.com/#website",
					"name": "Statesu",
					"url": "https://statesu.com/",
					"inLanguage": "en"
				},
				{
					"@type": "SoftwareApplication",
					"@id": "https://statesu.com/#application",
					"name": "Statesu",
					"url": "https://statesu.com/",
					"description": "Publish Markdown from a dashboard or CLI and add a blog to your existing website with an embed or API.",
					"applicationCategory": "DeveloperApplication",
					"operatingSystem": "Web",
					"sameAs": "https://github.com/burak-s/statesu"
				}
			]
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
		<h1>Add a Markdown blog<br />to your existing website.</h1>
		<p class="lead">Write in the dashboard or your terminal. Use an embed or the API to display posts without rebuilding your site.</p>
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

	<section>
		<h2>Publish on your own domain</h2>
		<p>Use the JavaScript embed for a quick setup. If your posts should show up in search and link previews, <a href="/docs/integration#search">serve each post as HTML with server-side rendering or static export</a>. See what that looks like in the <a href="/demo/blog/" rel="external">static HTML demo blog</a>. Either way, readers stay on your website.</p>
		<p>Use the hosted dashboard or <a href="https://github.com/burak-s/statesu#docker">self-host the open-source CMS</a> with a Rust binary and SQLite.</p>
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
