<script>
	let { children } = $props();
</script>

<header class="header">
	<a class="brand" href="/">Statesu</a>
	<nav aria-label="Documentation">
		<a href="/docs/cli">CLI</a>
		<a href="/docs/api">API</a>
		<a href="/docs/integration">Web integration</a>
		<a href="/dashboard">Dashboard</a>
		<a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a>
	</nav>
</header>

<main>
	{@render children()}
</main>

<footer>
	<a href="/">← Home</a>
	<a href="https://github.com/burak-s/statesu" target="_blank" rel="noopener noreferrer">GitHub</a>
</footer>
