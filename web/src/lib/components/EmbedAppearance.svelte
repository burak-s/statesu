<script>
	// Embed appearance editor: one saved appearance per blog, applied by the
	// v2 embed on every host page. Draft state is separate from the saved
	// value; presets fill the same fields the controls bind to, and the
	// preview renders through the embed's own modules so what the owner sees
	// here is what visitors get.
	import { embedStyles, parseAppearance, applyAppearance, RANGES } from '../../embed/appearance.js';
	import { buildMediaRenderer } from '../../embed/media-renderer.js';
	import { renderPost } from '../../embed/common.js';

	/**
	 * `blog` — the selected blog row (its `embed_appearance` seeds the draft);
	 * `save` — async (draft | null) => string | null, performs the API write.
	 */
	let { blog, save } = $props();

	const PRESETS = [
		{
			id: 'stock',
			label: 'Default',
			hint: 'The built-in embed look',
			value: null
		},
		{
			id: 'editorial',
			label: 'Editorial',
			hint: 'Generous line height and heading scale',
			value: {
				version: 1,
				typography: { line_height: 180, heading_scale: 120 },
				layout: { max_width: 720, spacing: 8 }
			}
		},
		{
			id: 'card',
			label: 'Card',
			hint: 'Bordered, padded panel',
			value: {
				version: 1,
				layout: { max_width: 760, padding: 24, spacing: 16 },
				decoration: { border_width: 1, corner_radius: 12 }
			}
		}
	];

	// Preview samples cover the structures the tokens touch: headings,
	// paragraph rhythm, lists, quotes, code, links and an image.
	const SAMPLE_POST = {
		title: 'Sample post',
		content_markdown:
			'A paragraph with a [link](https://example.com), some **bold** and *emphasis* text.\n\n' +
			'## Section heading\n\n' +
			'- First list item\n- Second list item\n\n' +
			'> A quoted line of text\n\n' +
			'```js\nconst ready = true;\n```\n\n' +
			`![Sample image](${new URL('/embed-preview.svg', window.location.origin).href})`
	};

	const SAMPLE_FEED = [
		{ title: 'First announcement', content_markdown: 'The first entry with a [link](https://example.com).' },
		{ title: 'Second announcement', content_markdown: 'The second entry, separated by the feed divider.' }
	];

	// Draft state ------------------------------------------------------------
	/** The in-progress appearance in wire shape (snake_case groups); null means stock. */
	/** @type {any} */
	let draft = $state(null);
	/** Which blog the draft belongs to; switching blogs never carries a draft over. */
	let draftBlogId = $state('');
	/** @type {string} */
	let saveError = $state('');
	let savedNote = $state(false);
	let saving = $state(false);
	let narrowPreview = $state(false);

	const savedAppearance = $derived(blog?.embed_appearance ?? null);

	// Re-seed the draft when another blog is selected. After a successful
	// save the same blog updates in place, so the draft survives.
	$effect(() => {
		const id = blog?.id ?? '';
		if (id !== draftBlogId) {
			draftBlogId = id;
			draft = toJsonSafe(savedAppearance);
			saveError = '';
			savedNote = false;
		}
	});

	/**
	 * Snapshot helper: `$state` proxies cannot be `structuredClone`d, but
	 * every value here is plain JSON data, so a JSON round-trip is both safe
	 * and detached from reactivity. Retired font choices are dropped so
	 * old saved appearances cannot reintroduce font selection.
	 * @param {any} appearance @returns {any}
	 */
	function toJsonSafe(appearance) {
		if (!appearance) return null;
		const copy = JSON.parse(JSON.stringify(appearance));
		if (copy.typography) {
			delete copy.typography.font;
			delete copy.typography.google_font;
			if (Object.keys(copy.typography).length === 0) delete copy.typography;
		}
		return copy;
	}

	/** @returns {any} */
	function ensureDraft() {
		if (!draft) draft = { version: 1 };
		return draft;
	}

	/**
	 * @param {string} group
	 * @param {string} key
	 * @param {string|number|null} value — '' unsets the token
	 */
	function setToken(group, key, value) {
		const current = ensureDraft();
		if (value === '' || value === null || value === undefined) {
			if (current[group]) {
				const next = { ...current[group] };
				delete next[key];
				if (Object.keys(next).length === 0) {
					const { [group]: _dropped, ...rest } = current;
					draft = Object.keys(rest).length ? rest : null;
					return;
				}
				draft = { ...current, [group]: next };
			}
			return;
		}
		draft = { ...current, [group]: { ...(current[group] ?? {}), [key]: value } };
	}

	/** @param {string} group @param {string} key @returns {string|number} */
	function token(group, key) {
		return /** @type {any} */ (draft)?.[group]?.[key] ?? '';
	}

	/** Presets replace the whole draft; that is what a preset means.
	 *  @param {{ value: any }} preset
	 */
	function applyPreset(preset) {
		draft = preset.value ? structuredClone(preset.value) : null;
		saveError = '';
		savedNote = false;
	}

	// Validation -------------------------------------------------------------
	const HEX_INPUT = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

	/** @type {{ group: 'colors'|'typography'|'layout'|'decoration', key: string, label: string }[]} */
	const TOKEN_FIELDS = [
		{ group: 'colors', key: 'text', label: 'Text color' },
		{ group: 'colors', key: 'background', label: 'Background' },
		{ group: 'colors', key: 'link', label: 'Link color' },
		{ group: 'colors', key: 'muted', label: 'Muted color' },
		{ group: 'colors', key: 'border', label: 'Border color' },
		{ group: 'typography', key: 'body_size', label: 'Body size (%)' },
		{ group: 'typography', key: 'heading_scale', label: 'Heading scale (%)' },
		{ group: 'typography', key: 'line_height', label: 'Line height (%)' },
		{ group: 'layout', key: 'max_width', label: 'Max width (px)' },
		{ group: 'layout', key: 'padding', label: 'Padding (px)' },
		{ group: 'layout', key: 'spacing', label: 'Spacing (px)' },
		{ group: 'decoration', key: 'border_width', label: 'Border width (px)' },
		{ group: 'decoration', key: 'corner_radius', label: 'Corner radius (px)' }
	];

	/** True when a token is present but malformed; such drafts cannot be saved.
	 *  @param {string} group @param {string} key
	 */
	function invalidToken(group, key) {
		const value = token(group, key);
		if (value === '') return false;
		if (group === 'colors') {
			if (key === 'background') {
				return typeof value === 'string' && value.toLowerCase() !== 'transparent' && !HEX_INPUT.test(value);
			}
			return !HEX_INPUT.test(String(value));
		}
		const [min, max] = RANGES[key];
		const numeric = Number(value);
		return !Number.isInteger(numeric) || numeric < min || numeric > max;
	}

	const hasInvalidTokens = $derived(TOKEN_FIELDS.some(({ group, key }) => invalidToken(group, key)));

	// Draft wiring shape for the API: number inputs deliver strings, so the
	// token values are converted to the integers the backend expects. An
	// unparsable entry stays a string and is rejected by validation instead.
	const draftForWire = $derived.by(() => {
		if (!draft) return null;
		const copy = toJsonSafe(draft);
		for (const { group, key } of TOKEN_FIELDS) {
			const value = copy[group]?.[key];
			if (typeof value === 'string' && value !== '' && group !== 'colors') {
				copy[group][key] = Number(value);
			}
		}
		return copy;
	});

	const dirty = $derived.by(() => {
		const left = JSON.stringify(normalize(draftForWire));
		const right = JSON.stringify(normalize(savedAppearance));
		return left !== right;
	});

	/** @param {any} appearance @returns {any} */
	function normalize(appearance) {
		const parsed = parseAppearance(toJsonSafe(appearance) ?? undefined);
		return parsed ?? null;
	}

	// Contrast feedback ------------------------------------------------------
	/** @param {string} hex */
	function luminance(hex) {
		let value = hex.slice(1);
		if (value.length === 3) value = value.split('').map((/** @type {string} */ ch) => ch + ch).join('');
		const channels = [0, 2, 4].map((offset) => {
			const channel = parseInt(value.slice(offset, offset + 2), 16) / 255;
			return channel <= 0.03928
				? channel / 12.92
				: Math.pow((channel + 0.055) / 1.055, 2.4);
		});
		return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
	}

	const contrastWarning = $derived.by(() => {
		const text = token('colors', 'text');
		const background = token('colors', 'background');
		if (!HEX_INPUT.test(String(text)) || !HEX_INPUT.test(String(background))) return '';
		const light = luminance(String(text));
		const dark = luminance(String(background));
		const ratio = (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05);
		return ratio < 4.5
			? `Text and background contrast is ${ratio.toFixed(1)}:1 — below the 4.5:1 accessibility minimum.`
			: '';
	});

	// Actions ----------------------------------------------------------------
	/** @param {string} group @param {string} key @param {any} event */
	function pickColor(group, key, event) {
		const value = /** @type {HTMLInputElement} */ (event.target).value;
		if (value) setToken(group, key, value);
	}

	async function saveDraft() {
		if (saving || hasInvalidTokens) return;
		saving = true;
		saveError = '';
		const error = await save(draftForWire);
		saving = false;
		if (error) {
			saveError = error;
			return;
		}
		savedNote = true;
	}

	function discardDraft() {
		draft = toJsonSafe(savedAppearance);
		saveError = '';
		savedNote = false;
	}

	// Preview ----------------------------------------------------------------
	/** @type {HTMLDivElement | null} */
	let previewHost = $state(null);
	let previewFeed = $state(false);

	// Build the shadow tree once.
	$effect(() => {
		if (!previewHost || previewHost.shadowRoot) return;
		const shadow = previewHost.attachShadow({ mode: 'open' });
		const style = document.createElement('style');
		style.textContent = embedStyles();
		shadow.appendChild(style);
		const root = document.createElement('article');
		root.className = 'statesu-v2 statesu-embed';
		shadow.appendChild(root);
	});

	// Samples mirror the embed's own structures: one post, or a feed of
	// announcements separated by the divider the spacing/border tokens
	// style. renderPost appends, so mode switches rebuild the content.
	$effect(() => {
		const root = /** @type {HTMLElement | null} */ (previewHost?.shadowRoot?.querySelector('.statesu-v2') ?? null);
		if (!root) return;
		const feed = previewFeed;
		root.textContent = '';
		const renderer = buildMediaRenderer(window, new Map());
		if (feed) {
			const container = document.createElement('div');
			container.className = 'statesu-feed';
			for (const post of SAMPLE_FEED) {
				const entry = document.createElement('article');
				entry.className = 'statesu-announcement';
				renderPost(window, entry, post, renderer, false);
				container.appendChild(entry);
			}
			root.appendChild(container);
		} else {
			renderPost(window, root, SAMPLE_POST, renderer, false);
		}
	});

	// Track the draft deeply so every token edit restyles the preview; the
	// stock look (no tokens) is the result when the draft cannot parse.
	const previewVars = $derived.by(() => {
		JSON.stringify(draftForWire);
		return parseAppearance(draftForWire ?? undefined);
	});

	$effect(() => {
		const root = /** @type {HTMLElement | null} */ (previewHost?.shadowRoot?.querySelector('.statesu-v2') ?? null);
		if (!root) return;
		for (const name of [...root.style]) {
			if (name.startsWith('--statesu-internal')) root.style.removeProperty(name);
		}
		applyAppearance(root, previewVars);
	});
</script>

<div class="appearance">
	<h3>Embed appearance</h3>
	<p>
		Style how your posts look inside the v2 embed — on every site that shows them, without touching
		any snippet. Unset settings keep the built-in look.
	</p>

	<div class="presets" role="group" aria-label="Presets">
		{#each PRESETS as preset (preset.id)}
			<button
				type="button"
				class="preset"
				title={preset.hint}
				onclick={() => applyPreset(preset)}
			>{preset.label}</button>
		{/each}
	</div>

	<div class="editor">
		<div class="controls">
			<h4>Colors</h4>
			{#each TOKEN_FIELDS.filter((field) => field.group === 'colors') as field (field.key)}
				<div class="token" class:invalid={invalidToken(field.group, field.key)}>
					<label for={`appearance-${field.key}`}>{field.label}</label>
					<input
						id={`appearance-${field.key}`}
						value={token(field.group, field.key)}
						placeholder={field.key === 'background' ? '#ffffff / transparent' : '#1a2b3c'}
						oninput={(event) => setToken(field.group, field.key, /** @type {HTMLInputElement} */ (event.target).value.trim())}
					/>
					<input
						type="color"
						tabindex="-1"
						aria-label={`${field.label} picker`}
						value={HEX_INPUT.test(String(token(field.group, field.key))) && String(token(field.group, field.key)).length === 7
							? String(token(field.group, field.key)).toLowerCase()
							: '#888888'}
						oninput={(event) => pickColor(field.group, field.key, event)}
					/>
				</div>
			{/each}

			<h4>Typography</h4>
			<p class="note">Fonts are inherited from your website. The embed does not load fonts.</p>
			{#each TOKEN_FIELDS.filter((field) => field.group === 'typography') as field (field.key)}
				<div class="token" class:invalid={invalidToken(field.group, field.key)}>
					<label for={`appearance-${field.key}`}>{field.label}</label>
					<input
						id={`appearance-${field.key}`}
						type="number"
						min={RANGES[field.key][0]}
						max={RANGES[field.key][1]}
						value={token(field.group, field.key)}
						placeholder={`${RANGES[field.key][0]}–${RANGES[field.key][1]}`}
						oninput={(event) => setToken(field.group, field.key, /** @type {HTMLInputElement} */ (event.target).value)}
					/>
				</div>
			{/each}

			<h4>Layout</h4>
			{#each TOKEN_FIELDS.filter((field) => field.group === 'layout') as field (field.key)}
				<div class="token" class:invalid={invalidToken(field.group, field.key)}>
					<label for={`appearance-${field.key}`}>{field.label}</label>
					<input
						id={`appearance-${field.key}`}
						type="number"
						min={RANGES[field.key][0]}
						max={RANGES[field.key][1]}
						value={token(field.group, field.key)}
						placeholder={`${RANGES[field.key][0]}–${RANGES[field.key][1]}`}
						oninput={(event) => setToken(field.group, field.key, /** @type {HTMLInputElement} */ (event.target).value)}
					/>
				</div>
			{/each}

			<h4>Decoration</h4>
			{#each TOKEN_FIELDS.filter((field) => field.group === 'decoration') as field (field.key)}
				<div class="token" class:invalid={invalidToken(field.group, field.key)}>
					<label for={`appearance-${field.key}`}>{field.label}</label>
					<input
						id={`appearance-${field.key}`}
						type="number"
						min={RANGES[field.key][0]}
						max={RANGES[field.key][1]}
						value={token(field.group, field.key)}
						placeholder={`${RANGES[field.key][0]}–${RANGES[field.key][1]}`}
						oninput={(event) => setToken(field.group, field.key, /** @type {HTMLInputElement} */ (event.target).value)}
					/>
				</div>
			{/each}
		</div>

		<div class="preview">
			<div class="preview-bar">
				<strong>Preview</strong>
				<span class="note">{draft ? 'Saved appearance' : 'Built-in look'}</span>
				<button
					type="button"
					class="width-toggle"
					onclick={() => (previewFeed = !previewFeed)}
				>{previewFeed ? 'Post' : 'Feed'}</button>
				<button type="button" class="width-toggle" onclick={() => (narrowPreview = !narrowPreview)}>
					{narrowPreview ? 'Desktop' : 'Mobile'}
				</button>
			</div>
			<!-- The preview is the embed's own renderer and stylesheet inside a
			     shadow root: it cannot drift from what visitors see. -->
			<div class="preview-frame" class:narrow={narrowPreview}>
				<div bind:this={previewHost} class="preview-host"></div>
			</div>
		</div>
	</div>

	{#if contrastWarning}
		<p class="warning" role="status">{contrastWarning}</p>
	{/if}
	{#if hasInvalidTokens}
		<p class="warning" role="status">Some values are outside their allowed range — fix them before saving.</p>
	{/if}

	<div class="actions">
		<button type="button" disabled={saving || hasInvalidTokens || !dirty} onclick={saveDraft}>
			{saving ? 'Saving…' : 'Save appearance'}
		</button>
		<button type="button" class="secondary" disabled={!dirty} onclick={discardDraft}>Discard changes</button>
		{#if dirty}
			<span class="note">Unsaved changes</span>
		{/if}
	</div>
	{#if savedNote && !dirty}
		<p class="ok" role="status">Appearance saved. Existing embeds pick it up on their next page load.</p>
	{/if}
	{#if saveError}
		<p class="error" role="alert">{saveError}</p>
	{/if}
</div>

<style>
	.appearance {
		display: grid;
		gap: 14px;
	}
	.presets {
		display: flex;
		gap: 8px;
		flex-wrap: wrap;
	}
	.preset {
		background: none;
		border: 1px solid var(--border, #ddd);
		padding: 6px 14px;
		font: inherit;
		font-size: 13px;
		cursor: pointer;
	}
	.editor {
		display: grid;
		grid-template-columns: minmax(240px, 320px) 1fr;
		gap: 24px;
		align-items: start;
	}
	@media (max-width: 720px) {
		.editor {
			grid-template-columns: 1fr;
		}
	}
	.controls {
		display: grid;
		gap: 8px;
	}
	.controls h4 {
		margin: 10px 0 2px;
	}
	.token {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 6px;
		align-items: center;
	}
	.token label {
		grid-column: 1 / -1;
		font-size: 12px;
	}
	.token.invalid input {
		outline: 2px solid #b91c1c;
	}
	.preview-bar {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-bottom: 8px;
	}
	.width-toggle {
		margin-left: auto;
		font: inherit;
		font-size: 12px;
		background: none;
		border: 1px solid var(--border, #ddd);
		padding: 3px 10px;
		cursor: pointer;
	}
	.preview-frame {
		border: 1px solid var(--border, #ddd);
		padding: 12px;
		overflow: auto;
	}
	.preview-frame.narrow {
		max-width: 380px;
	}
	.preview-host {
		min-height: 120px;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.secondary {
		background: none;
	}
	.warning {
		color: #92400e;
	}
</style>
