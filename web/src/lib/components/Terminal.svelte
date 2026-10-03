<script>
	let { code = '', variants, language = 'bash' } = $props();
	const id = $props.id();
	let selected = $state('curl');
	const tools = $derived(Object.keys(variants ?? {}));
	const active = $derived(tools.includes(selected) ? selected : tools[0]);
	const displayedCode = $derived(variants ? variants[active] : code);

	function navigateTabs(event) {
		const index = tools.indexOf(active);
		let next;
		if (event.key === 'ArrowRight') next = (index + 1) % tools.length;
		else if (event.key === 'ArrowLeft') next = (index - 1 + tools.length) % tools.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = tools.length - 1;
		else return;
		event.preventDefault();
		selected = tools[next];
		event.currentTarget.parentElement.querySelectorAll('button')[next].focus();
	}

	// Keep the original text intact and let Svelte escape every token.
	const tokens = $derived.by(() => {
		const pattern = /(?<comment>^[ \t]*#.*$)|(?<key>"(?:\\.|[^"\\])*"(?=\s*:))|(?<string>'[^']*'|"(?:\\.|[^"\\])*")|(?<variable>\$[A-Za-z_][\w]*|\b[A-Z_][A-Z_0-9]*(?==))|(?<flag>--?[A-Za-z][\w-]*)|(?<command>\b(?:curl|wget|jq|read|printf|unset|date)\b)|(?<number>\b\d+\b)|(?<operator>[|\\]|\$\(|[{}])/gm;
		const result = [];
		let end = 0;
		for (const match of displayedCode.matchAll(pattern)) {
			if (match.index > end) result.push({ text: displayedCode.slice(end, match.index) });
			result.push({ text: match[0], kind: Object.keys(match.groups).find((key) => match.groups[key] !== undefined) });
			end = match.index + match[0].length;
		}
		if (end < displayedCode.length) result.push({ text: displayedCode.slice(end) });
		return result;
	});
</script>

<div class="terminal">
	<div class="terminal-bar">
		{#if variants}
			<div class="tabs" role="tablist" aria-label="HTTP client">
				{#each tools as tool}
					<button type="button" role="tab" id={`${id}-${tool}`} aria-selected={active === tool} aria-controls={`${id}-panel`} tabindex={active === tool ? 0 : -1} onclick={() => selected = tool} onkeydown={navigateTabs}>{tool}</button>
				{/each}
			</div>
		{:else}
			<span>{language === 'json' ? 'JSON response' : 'Terminal'}</span>
		{/if}
		<span>{language}</span>
	</div>
	<!-- Keyboard focus lets readers scroll long commands without a mouse. -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<pre id={`${id}-panel`} tabindex="0" role={variants ? 'tabpanel' : 'region'} aria-labelledby={variants ? `${id}-${active}` : undefined} aria-label={variants ? undefined : language === 'json' ? 'JSON response example' : 'Terminal command example'}><code>{#each tokens as token}<span class={token.kind}>{token.text}</span>{/each}</code></pre>
</div>

<style>
	.terminal {
		margin: 24px 0;
		background: #171d29;
		color: #e8edf5;
		border-radius: 8px;
		overflow: hidden;
	}
	.terminal-bar {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 16px;
		padding: 12px 22px;
		background: #222b3b;
		color: #bac6d9;
		font-family: ui-monospace, 'SFMono-Regular', Menlo, Consolas, monospace;
		font-size: 11px;
		letter-spacing: .04em;
	}
	.tabs { display: flex; gap: 20px; }
	.tabs button {
		font: inherit;
		font-size: 13px;
		color: #bac6d9;
		background: none;
		border: 0;
		border-bottom: 2px solid transparent;
		padding: 4px 0;
		cursor: pointer;
	}
	.tabs button[aria-selected='true'] { color: #82e0b0; border-bottom-color: #82e0b0; }
	.tabs button:hover { color: #e8edf5; }
	.tabs button:focus-visible { outline-color: #8dc9ff; }
	pre:focus-visible { outline-color: #8dc9ff; outline-offset: -3px; }
	pre {
		margin: 0;
		padding: 22px;
		overflow-x: auto;
		line-height: 1.85;
		tab-size: 2;
	}
	code { color: inherit; font-size: 13px; overflow-wrap: normal; }
	.comment { color: #a5b2c7; font-style: italic; }
	.command { color: #82e0b0; font-weight: 700; }
	.flag, .key { color: #8dc9ff; }
	.string { color: #f0d48d; }
	.variable { color: #d5b2ff; }
	.number { color: #ffbb96; }
	.operator { color: #8de0e5; }
	@media (max-width: 540px) {
		pre { padding: 18px 16px; }
		.terminal-bar { padding-inline: 16px; }
	}
</style>
