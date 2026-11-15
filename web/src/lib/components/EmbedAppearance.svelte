<script>
	// Embed appearance editor: one saved appearance per blog, applied by the
	// v2 embed on every host page. Draft state is separate from the saved
	// value; presets fill the same fields the controls bind to, and the
	// preview renders through the embed's own modules so what the owner sees
	// here is what visitors get.
	import { embedStyles, parseAppearance, applyAppearance, RANGES, MARKDOWN_FIELDS, validateAppearanceDocument, appearanceTemplate, stripDefaults } from '../../embed/appearance.js';
	import { buildMediaRenderer } from '../../embed/media-renderer.js';
	import { renderPost } from '../../embed/common.js';
	import { request } from '#lib/api.js';

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
			'# First-level heading\n\n## Section heading\n\n### Smaller heading\n\n#### Fourth-level heading\n\n##### Fifth-level heading\n\n###### Sixth-level heading\n\nInline `code` stays separate from fenced code blocks.\n\n' +
			'- First list item\n  - Nested item\n- Second list item\n\n1. Ordered item\n2. Another item\n\n' +
			'> A quoted line of text\n\n' +
			'```js\nconst ready = true;\n```\n\n' +
			'| Feature | Status |\n| --- | --- |\n| Markdown | Ready |\n| Styling | Customizable |\n\n---\n\n' +
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
	let editorMode = $state('fields');
	let jsonText = $state('null');
	let jsonError = $state('');
	let mobilePanel = $state('edit');
	let previewPosts = $state([]);
	let previewPostId = $state('');
	let previewPage = $state(0);
	let previewTotal = $state(0);
	let postsLoading = $state(false);
	let postsError = $state('');
	/** @type {any[]} */
	let variants = $state([]);
	let variantsLoading = $state(false);
	let variantsError = $state('');
	let variantName = $state('');
	let variantNote = $state('');
	let variantBusy = $state(false);

	async function loadVariants() {
		const id = blog?.id;
		if (!id) return;
		variantsLoading = true;
		variantsError = '';
		try {
			const result = await request(`/blogs/${id}/appearance/variants`);
			if (blog?.id !== id) return;
			if (result.ok) {
				variants = result.data;
			} else {
				variantsError = result.error;
			}
		} catch {
			if (blog?.id === id) variantsError = 'Could not load saved styles. Try again.';
		} finally {
			if (blog?.id === id) variantsLoading = false;
		}
	}

	/** Snapshot the current draft (or the saved look) under a name. */
	async function saveVariant() {
		const id = blog?.id;
		const name = variantName.trim();
		if (!id || !name || variantBusy || jsonError || hasInvalidTokens) return;
		variantBusy = true;
		variantNote = '';
		variantsError = '';
		try {
			const result = await request(`/blogs/${id}/appearance/variants`, {
				method: 'POST',
				body: { name, embed_appearance: draftForWire }
			});
			if (blog?.id !== id) return;
			if (result.ok) {
				variants = [result.data, ...variants.filter((variant) => variant.id !== result.data.id)];
				variantName = '';
				variantNote = `Style “${result.data.name}” saved.`;
			} else {
				variantsError = result.error;
			}
		} catch {
			if (blog?.id === id) variantsError = 'Could not save the style. Try again.';
		} finally {
			if (blog?.id === id) variantBusy = false;
		}
	}

	/** Applies a saved style: persists it and adopts it as the draft. */
	async function applyVariant(variant) {
		const id = blog?.id;
		if (!id || saving || variantBusy || jsonError || hasInvalidTokens) return;
		if (dirty && !window.confirm('Applying a saved style replaces your unsaved changes. Continue?')) return;
		variantBusy = true;
		saveError = '';
		try {
			const error = await save(toJsonSafe(variant.embed_appearance));
			if (blog?.id !== id) return;
			if (error) {
				saveError = error;
				return;
			}
			draft = toJsonSafe(variant.embed_appearance);
			syncJson();
			savedNote = true;
		} catch {
			saveError = 'Could not apply the style. Try again.';
		} finally {
			if (blog?.id === id) variantBusy = false;
		}
	}

	async function deleteVariant(variant) {
		const id = blog?.id;
		if (!id || variantBusy) return;
		if (!window.confirm(`Delete the saved style “${variant.name}”?`)) return;
		variantBusy = true;
		variantsError = '';
		try {
			const result = await request(`/blogs/${id}/appearance/variants/${variant.id}`, { method: 'DELETE' });
			if (blog?.id !== id) return;
			if (result.ok) {
				variants = variants.filter((candidate) => candidate.id !== variant.id);
				variantNote = '';
			} else {
				variantsError = result.error;
			}
		} catch {
			if (blog?.id === id) variantsError = 'Could not delete the style. Try again.';
		} finally {
			if (blog?.id === id) variantBusy = false;
		}
	}



	async function loadPreviewPosts() {
		if (postsLoading || !blog?.id) return;
		const id = blog.id;
		postsLoading = true;
		postsError = '';
		try {
			const result = await request(`/blogs/${id}/posts?page=${previewPage + 1}&size=20`);
			if (blog?.id !== id) return;
			if (!result.ok) { postsError = result.error; return; }
			previewPosts = [...previewPosts, ...result.data.items];
			previewPage += 1;
			previewTotal = result.data.total;
		} catch {
			if (blog?.id === id) postsError = 'Could not load posts. Try again.';
		} finally {
			if (blog?.id === id) postsLoading = false;
		}
	}

	function switchMode(mode) {
		if (mode === editorMode || jsonError || hasInvalidTokens) return;
		if (mode === 'json') jsonText = JSON.stringify(appearanceTemplate(draftForWire), null, 2);
		editorMode = mode;
	}

	function editJson(text) {
		jsonText = text;
		savedNote = false;
		try {
			const value = JSON.parse(text);
			jsonError = validateAppearanceDocument(value);
			if (!jsonError) draft = value === null ? null : stripDefaults(value);
		} catch {
			jsonError = 'Invalid JSON. Check quotes, commas and brackets. Preview shows the last valid draft.';
		}
	}

	function syncJson() {
		jsonText = JSON.stringify(appearanceTemplate(draft), null, 2);
		jsonError = '';
	}

	const savedAppearance = $derived(blog?.embed_appearance ?? null);

	// Re-seed the draft when another blog is selected. After a successful
	// save the same blog updates in place, so the draft survives.
	$effect(() => {
		const id = blog?.id ?? '';
		if (id !== draftBlogId) {
			draftBlogId = id;
			draft = toJsonSafe(savedAppearance);
			syncJson();
			previewPosts = [];
			previewPostId = '';
			previewPage = 0;
			previewTotal = 0;
			postsLoading = false;
			postsError = '';
			saveError = '';
			savedNote = false;
			variants = [];
			variantsError = '';
			variantName = '';
			variantNote = '';
			loadVariants();
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
		for (const group of ['colors', 'typography', 'layout', 'decoration', 'markdown']) {
			if (copy[group] === null) delete copy[group];
			else if (copy[group]) {
				for (const key of Object.keys(copy[group])) if (copy[group][key] === null) delete copy[group][key];
				if (Object.keys(copy[group]).length === 0) delete copy[group];
			}
		}
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
		savedNote = false;
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
		if ((dirty || jsonError || hasInvalidTokens) && !window.confirm('Replace your unsaved appearance changes with this preset?')) return;
		draft = preset.value ? structuredClone(preset.value) : null;
		syncJson();
		saveError = '';
		savedNote = false;
	}

	// Validation -------------------------------------------------------------
	const HEX_INPUT = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;

	/** @type {any[]} */
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
		{ group: 'decoration', key: 'corner_radius', label: 'Corner radius (px)' },
		...MARKDOWN_FIELDS.map((field) => ({ ...field, group: 'markdown' }))
	].map((field) => ({ ...field,
		color: field.group === 'colors' || field.color,
		section: field.section ?? ({ colors: 'Colors', typography: 'Typography', layout: 'Outer container', decoration: 'Container border' })[field.group]
	}));

	function defaultHint(field) {
		const defaults = {
			body_size: '100%', heading_scale: '100%', line_height: '160%', max_width: 'No limit',
			padding: '0px', border_width: '0px', corner_radius: '0px', background: 'Transparent',
			text: 'Website text color', link: 'Browser link color', muted: 'Text color', border: 'rgba(0,0,0,.15)',
			spacing: 'Browser block spacing', paragraph_spacing: 'Global / browser spacing', heading_spacing: 'Browser heading spacing',
			list_indent: '40px', list_spacing: '0px', link_decoration: 'Underline', link_hover_decoration: 'Link underline setting',
			link_hover_color: 'Link color', table_border_color: 'Transparent', table_cell_padding: '1px', image_width: 'Natural / Markdown-pinned width',
			image_align: 'Center (unlinked images)', image_spacing: '1.25em', rule_width: '1px', rule_color: 'Text color'
		};
		if (defaults[field.key]) return defaults[field.key];
		if (/^h[1-6]_size$/.test(field.key)) return 'Browser heading size × heading scale';
		if (/^h[1-6]_weight$/.test(field.key)) return 'Bold (700)';
		if (/^h[1-6]_spacing$/.test(field.key)) return 'Heading / browser spacing';
		if (field.key.endsWith('_background')) return 'Transparent';
		if (field.key.endsWith('_color')) return 'Inherited text color';
		if (field.key.endsWith('_size')) return 'Browser monospace size';
		return '0px';
	}

	/** True when a token is present but malformed; such drafts cannot be saved.
	 *  @param {string} group @param {string} key
	 */
	function invalidToken(group, key) {
		const value = token(group, key);
		if (value === '') return false;
		const options = TOKEN_FIELDS.find((field) => field.group === group && field.key === key)?.options;
		if (options) return !options.includes(value);
		if (group === 'colors' || (group === 'markdown' && MARKDOWN_FIELDS.some((field) => field.key === key && field.color))) {
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
		for (const { group, key, color, options } of TOKEN_FIELDS) {
			const value = copy[group]?.[key];
			if (typeof value === 'string' && value !== '' && group !== 'colors' && !color && !options) {
				copy[group][key] = Number(value);
			}
		}
		return copy;
	});

	const dirty = $derived.by(() => {
		if (jsonError || hasInvalidTokens) return true;
		const left = JSON.stringify(normalize(draftForWire));
		const right = JSON.stringify(normalize(savedAppearance));
		return left !== right;
	});

	/** @param {any} appearance @returns {any} */
	function normalize(appearance) {
		const parsed = parseAppearance(toJsonSafe(appearance) ?? undefined);
		return parsed && Object.values(parsed).some((group) => group && Object.keys(group).length > 0) ? parsed : null;
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
		if (saving || hasInvalidTokens || jsonError) return;
		saving = true;
		saveError = '';
		try {
			const error = await save(draftForWire);
			if (error) { saveError = error; return; }
			savedNote = true;
		} catch {
			saveError = 'Could not save appearance. Your draft is preserved; try again.';
		} finally {
			saving = false;
		}
	}

	function discardDraft() {
		draft = toJsonSafe(savedAppearance);
		syncJson();
		saveError = '';
		savedNote = false;
	}

	// Preview ----------------------------------------------------------------
	/** @type {HTMLDivElement | null} */
	let previewHost = $state(null);
	let previewFeed = $state(false);
	let previewSection = $state('');

	const SECTION_TARGETS = {
		Text: 'h2', Lists: 'ul', 'Code blocks': 'pre', 'Inline code': 'p code',
		Blockquotes: 'blockquote', Tables: 'table', Images: 'img', Dividers: 'hr', Links: 'a',
		H1: '.statesu-body h1', H2: '.statesu-body h2', H3: 'h3', H4: 'h4', H5: 'h5', H6: 'h6'
	};

	function focusSection(section) {
		previewFeed = false;
		previewSection = section;
	}

	$effect(() => {
		const section = previewSection;
		const feed = previewFeed;
		if (!section || feed || !previewHost) return;
		const host = previewHost;
		const frame = requestAnimationFrame(() => {
			const target = host.shadowRoot?.querySelector(SECTION_TARGETS[section] ?? '.statesu-title');
			const viewport = host.closest('.preview-frame');
			if (target && viewport) {
				viewport.scrollTop += target.getBoundingClientRect().top - viewport.getBoundingClientRect().top - 16;
			}
		});
		return () => cancelAnimationFrame(frame);
	});

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
			renderPost(window, root, previewPosts.find((post) => post.id === previewPostId) ?? SAMPLE_POST, renderer, false);
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
	<h2>Embed appearance</h2>
	<p>Style every post in this blog’s v2 embeds without changing your Markdown files. Blank fields use built-in defaults; website overrides take priority.</p>

	<div class="presets" role="group" aria-label="Presets">
		{#each PRESETS as preset (preset.id)}
			<button
				type="button"
				class="preset"
				disabled={saving}
				title={preset.hint}
				onclick={() => applyPreset(preset)}
			>{preset.label}</button>
		{/each}
	</div>

	<div class="mode-bar" role="group" aria-label="Appearance editor mode">
		<button type="button" aria-pressed={editorMode === 'fields'} disabled={saving || !!jsonError || hasInvalidTokens} onclick={() => switchMode('fields')}>Fields</button>
		<button type="button" aria-pressed={editorMode === 'json'} disabled={saving || hasInvalidTokens} onclick={() => switchMode('json')}>JSON</button>
		<a href="/docs/integration" target="_blank" rel="noreferrer">Token reference</a>
	</div>
	<div class="variants">
		<h3>Saved styles</h3>
		<p class="note">Name the styling you see now — preview included — and switch between styles any time. Applying one persists it for every embed; deleting only removes the snapshot.</p>
		<div class="variant-save">
			<input
				aria-label="Style name"
				placeholder="Style name, e.g. Cozy winter"
				maxlength="64"
				bind:value={variantName}
				onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); saveVariant(); } }}
			/>
			<button
				type="button"
				disabled={variantBusy || saving || !!jsonError || hasInvalidTokens || variantName.trim() === ''}
				onclick={saveVariant}
			>{variantBusy ? 'Saving…' : 'Save current as style'}</button>
		</div>
		{#if variantNote}<p class="ok" role="status">{variantNote}</p>{/if}
		{#if variantsError}<p class="error" role="alert">{variantsError}</p>{/if}
		{#if variantsLoading}
			<p class="note">Loading saved styles…</p>
		{:else if variants.length === 0}
			<p class="note">No saved styles yet.</p>
		{:else}
			<ul class="variant-list">
				{#each variants as variant (variant.id)}
					<li>
						<strong>{variant.name}</strong>
						<button type="button" class="secondary" disabled={variantBusy || saving || !!jsonError || hasInvalidTokens} onclick={() => applyVariant(variant)}>Apply</button>
						<button type="button" class="secondary" disabled={variantBusy} onclick={() => deleteVariant(variant)}>Delete</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>

	<div class="mobile-tabs" role="group" aria-label="Appearance workspace">
		<button type="button" aria-pressed={mobilePanel === 'edit'} onclick={() => mobilePanel = 'edit'}>Edit styles</button>
		<button type="button" aria-pressed={mobilePanel === 'preview'} onclick={() => mobilePanel = 'preview'}>See preview</button>
	</div>
	<div class="editor" class:json-mode={editorMode === 'json'} class:show-preview={mobilePanel === 'preview'}>
		<fieldset class="controls" disabled={saving}>
			{#if editorMode === 'json'}
				<label for="appearance-json">Appearance JSON</label>
				<p class="note" id="json-help">Every supported field is listed with its default already filled in. Change a value to customize it; type the default back to restore it. Keywords name inherited defaults: inherit, revert, currentcolor, transparent and none follow your website or the built-in look and are dropped on save. Ranges are described in Fields and the token reference. This is the same document used by the CLI, without an embed_appearance wrapper. To clear all styling, save null.</p>
				<textarea id="appearance-json" aria-describedby="json-help json-feedback" aria-invalid={!!jsonError} spellcheck="false" value={jsonText} oninput={(event) => editJson(event.currentTarget.value)}></textarea>
				<p id="json-feedback" class:error={!!jsonError} role="status">{jsonError || 'Valid appearance JSON'}</p>
				<button type="button" class="secondary" disabled={!!jsonError} onclick={() => jsonText = JSON.stringify(appearanceTemplate(draftForWire), null, 2)}>Format JSON</button>
			{:else}
				{#each [...new Set(TOKEN_FIELDS.map((field) => field.section))] as section}
					<details open={section === 'Colors'} ontoggle={(event) => { if (event.currentTarget.open && SECTION_TARGETS[section]) focusSection(section); }}>
						<summary>{section}</summary>
						{#if section === 'Typography'}<p class="note">Fonts inherit from your website. No fonts are downloaded.</p>{/if}
						{#each TOKEN_FIELDS.filter((field) => field.section === section) as field (field.key)}
							<div class="token" class:invalid={invalidToken(field.group, field.key)}>
								<label for={`appearance-${field.key}`}>{field.label}{field.group === 'markdown' && !field.color && !field.options && field.unit ? ` (${field.unit})` : ''}</label>
								{#if field.options}
									<select id={`appearance-${field.key}`} value={token(field.group, field.key)} onchange={(event) => setToken(field.group, field.key, event.currentTarget.value)}>
										<option value="">Default</option>
										{#each field.options as option}<option value={option}>{option}</option>{/each}
									</select>
								{:else}
									<input id={`appearance-${field.key}`} type={field.color ? 'text' : 'number'}
										min={RANGES[field.key]?.[0]} max={RANGES[field.key]?.[1]}
										value={token(field.group, field.key)} aria-invalid={invalidToken(field.group, field.key)}
										placeholder={field.color ? field.key === 'background' ? '#rrggbb / transparent' : '#rrggbb' : `${RANGES[field.key][0]}–${RANGES[field.key][1]}`}
										oninput={(event) => setToken(field.group, field.key, event.currentTarget.value.trim())} />
								{/if}
								{#if field.color}
									<input type="color" aria-label={`${section}: ${field.label} picker`}
										value={/^#[0-9a-f]{6}$/i.test(String(token(field.group, field.key))) ? String(token(field.group, field.key)) : '#888888'}
										oninput={(event) => pickColor(field.group, field.key, event)} />
								{/if}
								<div class="field-help">
									<span class="note">Default: {defaultHint(field)}</span>
									<button type="button" class="secondary reset" aria-label={`Reset ${section}: ${field.label}`} disabled={token(field.group, field.key) === ''} onclick={() => setToken(field.group, field.key, '')}>Reset</button>
								</div>
							</div>
						{/each}
					</details>
				{/each}
			{/if}
		</fieldset>

		<div class="preview">
			<div class="preview-bar">
				<strong>Preview</strong>
				<span class="note">{dirty ? 'Unsaved changes' : draft ? 'Saved appearance' : 'Built-in look'}</span>
				<button
					type="button"
					class="width-toggle"
					onclick={() => (previewFeed = !previewFeed)}
				>{previewFeed ? 'Post' : 'Feed'}</button>
				<button type="button" class="width-toggle" onclick={() => (narrowPreview = !narrowPreview)}>
					{narrowPreview ? 'Desktop' : 'Mobile'}
				</button>
			</div>
			<label for="appearance-preview-post">Preview content</label>
			<select id="appearance-preview-post" bind:value={previewPostId} onchange={() => { previewFeed = false; }}>
				<option value="">Sample Markdown</option>
				{#each previewPosts as post (post.id)}<option value={post.id}>{post.title} ({post.status})</option>{/each}
			</select>
			{#if previewPage === 0 || previewPosts.length < previewTotal}
				<button type="button" class="secondary" disabled={postsLoading} onclick={loadPreviewPosts}>{postsLoading ? 'Loading posts…' : previewPage ? 'Load more posts' : 'Load my posts'}</button>
			{:else if previewPosts.length === 0}
				<p class="note">No posts yet. Use the sample to try every Markdown element.</p>
			{/if}
			{#if postsError}<p class="error" role="alert">{postsError}</p>{/if}
			<p class="note">Open a section to jump to that element, if present. Fonts inherit from this page; your website may use different fonts. Feed mode uses sample announcements.</p>
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
		<button type="button" disabled={saving || hasInvalidTokens || !!jsonError || !dirty} onclick={saveDraft}>
			{saving ? 'Saving…' : 'Save appearance'}
		</button>
		<button type="button" class="secondary" disabled={saving || !dirty} onclick={discardDraft}>Discard changes</button>
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
	.appearance { display: grid; gap: 16px; min-width: 0; color: #000; }
	h2 { margin: 0; font-size: 18px; font-weight: 650; letter-spacing: -.02em; }
	p { margin: 0; font-size: 14px; color: inherit; line-height: 1.6; }
	button { font: inherit; font-size: 13px; font-weight: 600; line-height: 1.5; min-height: 40px; padding: 12px 18px; background: #000; color: #fff; border: 0; border-radius: 0; cursor: pointer; }
	button:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 4px; }
	button:disabled { opacity: .5; cursor: not-allowed; }
	:focus-visible { outline: 2px solid #171717; outline-offset: 3px; }
	input, select, textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 12px 14px; font: inherit; font-size: 14px; line-height: 1.5; color: #fff; background: #000; border: 0; border-radius: 0; }
	input::placeholder { color: #fff; opacity: .7; }
	input[type='color'] { width: 44px; height: 45px; padding: 4px; cursor: pointer; }
	.note { font-size: 13px; }
	.error { background: #000; color: #fff; padding: 12px 16px; }
	.ok { font-weight: 600; }
	.presets {
		display: flex;
		gap: 24px;
		flex-wrap: wrap;
	}
	.preset {
		background: none;
		color: #000;
		padding: 4px 0;
		text-decoration: underline;
		text-underline-offset: 4px;
	}
	.variants { display: grid; gap: 10px; }
	.variants h3 { margin: 0; font-size: 14px; font-weight: 650; }
	.variant-save { display: flex; gap: 8px; flex-wrap: wrap; }
	.variant-save input { flex: 1 1 200px; }
	.variant-list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
	.variant-list li { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
	.variant-list strong { margin-right: auto; }
	.mode-bar { display: flex; align-items: center; gap: 12px; }
	.mode-bar button[aria-pressed='false'], .mobile-tabs button[aria-pressed='false'] { background: #eee; color: #000; }
	.mode-bar a { color: inherit; font-size: 13px; }
	.mobile-tabs { display: none; }
	textarea { min-height: 440px; resize: vertical; font-family: monospace; tab-size: 2; }
	.field-help { grid-column: 1 / -1; display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
	.reset { font-size: 12px; min-height: 32px; }
	.editor {
		display: grid;
		grid-template-columns: minmax(200px, 280px) minmax(0, 1fr);
		gap: 24px;
		align-items: start;
	}
	.editor.json-mode { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
	@media (max-width: 760px) {
		.editor, .editor.json-mode { grid-template-columns: 1fr; }
		.mobile-tabs { display: flex; gap: 8px; position: sticky; top: 0; background: #fff; padding: 8px 0; z-index: 2; }
		.editor:not(.show-preview) .preview { display: none; }
		.editor.show-preview .controls { display: none; }
	}
	.controls {
		border: 0; padding: 0; margin: 0; min-width: 0;
		display: grid;
		gap: 8px;
	}
	details { border-bottom: 1px solid #ddd; padding-bottom: 12px; }
	summary { cursor: pointer; padding: 12px 0; font-size: 14px; font-weight: 650; }
	details .token { margin-top: 12px; }
	.token {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 6px;
		align-items: center;
	}
	.token label {
		grid-column: 1 / -1;
		font-size: 13px;
		font-weight: 600;
	}
	.token.invalid input {
		outline: 2px solid #171717;
	}
	.preview-bar {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 10px;
		margin-bottom: 12px;
		font-size: 13px;
	}
	.width-toggle { font-size: 13px; background: none; color: #000; padding: 4px 0; min-height: 32px; text-decoration: underline; text-underline-offset: 4px; }
	.preview { min-width: 0; position: sticky; top: 16px; }
	@media (max-width: 760px) {
		.preview { position: static; }
	}
	.preview-frame {
		padding: 16px;
		border: 1px solid #ddd;
		max-height: 70vh;
		overflow: auto;
	}
	.preview-frame.narrow {
		max-width: 380px;
	}
	.preview-host {
		min-height: 120px;
	}
	.actions {
		position: sticky;
		bottom: 0;
		background: #fff;
		padding: 12px 0;
		z-index: 1;
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.secondary { background: none; color: #000; padding: 4px 0; text-decoration: underline; text-underline-offset: 4px; }
	.warning { color: #000; font-weight: 600; }
</style>
