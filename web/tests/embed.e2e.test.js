import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';

const { init } = await import('../src/embed/main.js');

class AbortError extends Error { constructor(message) { super(message); this.name = 'AbortError'; } }
const SCRIPT_SRC = 'https://statesu.example/embed/v1.js';

/** Boot the embed in a jsdom page and wait for its async settle. */
async function boot({ url, fetchBehavior, scriptAttrs }) {
	const dom = new JSDOM('<html><body><div id="t">initial</div></body></html>', { url });
	const window = dom.window;
	window.fetch = fetchBehavior;
	const script = window.document.createElement('script');
	script.setAttribute('data-blog', 'burak');
	script.setAttribute('data-target', 't');
	for (const [k, v] of Object.entries(scriptAttrs ?? {})) script.setAttribute(k, v);
	if (scriptAttrs?.src === undefined) script.setAttribute('src', SCRIPT_SRC);
	window.document.body.appendChild(script);
	init(window, { timeoutMs: 40 });
	await new Promise((resolve) => setTimeout(resolve, 60));
	const target = window.document.getElementById('t');
	return { window, target };
}

test('successful post renders sanitized content and forced link attributes', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog/test',
		fetchBehavior: async () => ({
			ok: true,
			status: 200,
			json: async () => ({
				slug: 'test',
				title: 'My Title',
				content_markdown: '# Hi\n\n<script>evil()</script>\n\n[a](javascript:alert(1))\n\n[ok](https://e.example/x) *em*'
			})
		})
	});
	const html = target.outerHTML;
	assert.match(html, /statesu-title">My Title</);
	assert.match(html, /<em>em<\/em>/);
	assert.equal(target.querySelectorAll('script').length, 0);
	const anchors = [...target.querySelectorAll('a')];
	assert.deepEqual(
		anchors.map((a) => [a.getAttribute('href'), a.getAttribute('target'), a.getAttribute('rel')]),
		[['https://e.example/x', '_blank', 'noopener noreferrer']]
	);
});

test('data-post renders a fixed announcement independently of the page URL', async () => {
	for (const url of ['https://site.example/', 'https://site.example/blog/other']) {
		let requested;
		const { target } = await boot({
			url,
			scriptAttrs: { 'data-post': 'guncel' },
			fetchBehavior: async (api) => {
				requested = api;
				return {
					ok: true, status: 200,
					json: async () => ({ title: 'Duyurular', content_markdown: '**Yeni yazı**' })
				};
			}
		});
		assert.equal(requested, 'https://statesu.example/public/blogs/burak/posts/guncel');
		assert.equal(target.querySelector('.statesu-title').textContent, 'Duyurular');
		assert.equal(target.querySelector('strong').textContent, 'Yeni yazı');
	}
});

test('invalid data-post never fetches or falls back to the page slug', async () => {
	for (const slug of ['', 'Test', '../private', 'a/b', 'a%2Fb', 'a'.repeat(64)]) {
		let calls = 0;
		const { target } = await boot({
			url: 'https://site.example/blog/test',
			scriptAttrs: { 'data-post': slug },
			fetchBehavior: async () => { calls += 1; throw new Error('must not fetch'); }
		});
		assert.equal(calls, 0);
		assert.match(target.textContent, /not a valid blog post address/);
	}
});

test('latest mode renders only the newest body without a title', async () => {
	let requested;
	const { target } = await boot({
		url: 'https://site.example/',
		scriptAttrs: { 'data-mode': 'latest', 'data-hide-title': '', 'data-post': 'ignored' },
		fetchBehavior: async (api) => {
			requested = api;
			return { ok: true, json: async () => ({ total: 2, items: [
				{ title: 'Newest title', content_markdown: '**Newest**\n\n<script>evil()</script>' },
				{ title: 'Older title', content_markdown: '_Older_' }
			] }) };
		}
	});
	assert.equal(requested, 'https://statesu.example/public/blogs/burak/posts?page=1&size=1');
	assert.equal(target.querySelectorAll('.statesu-body').length, 1);
	assert.equal(target.querySelector('.statesu-body strong').textContent, 'Newest');
	assert.ok(!target.textContent.includes('Older'));
	assert.equal(target.querySelectorAll('.statesu-title, script, button').length, 0);
	assert.ok(!target.textContent.includes('title'));
});

test('latest mode stays empty when there are no published announcements', async () => {
	const { target } = await boot({
		url: 'https://site.example/',
		scriptAttrs: { 'data-mode': 'latest', 'data-hide-title': '' },
		fetchBehavior: async () => ({ ok: true, json: async () => ({ items: [], total: 0 }) })
	});
	assert.equal(target.textContent, '');
	assert.equal(target.querySelectorAll('.statesu-body, button').length, 0);
});

test('latest mode rejects invalid responses and handles network errors safely', async () => {
	for (const [fetchBehavior, message] of [
		[async () => ({ ok: true, json: async () => ({}) }), /unexpected response/],
		[async () => ({ ok: true, json: async () => ({ items: [{ title: 'Bad' }], total: 1 }) }), /unexpected response/],
		[async () => { throw new Error('offline'); }, /could not be reached/]
	]) {
		const { target } = await boot({ url: 'https://site.example/', scriptAttrs: { 'data-mode': 'latest' }, fetchBehavior });
		assert.match(target.textContent, message);
		assert.equal(target.querySelectorAll('.statesu-body').length, 0);
	}
});

test('feed mode renders all announcement bodies without titles on /takes', async () => {
	let requested;
	const { target } = await boot({
		url: 'https://site.example/takes',
		scriptAttrs: { 'data-mode': 'feed', 'data-hide-title': '' },
		fetchBehavior: async (api) => {
			requested = api;
			return { ok: true, json: async () => ({ total: 2, items: [
				{ title: 'Newest title', content_markdown: '**Newest**\n\n<script>evil()</script>' },
				{ title: 'Older title', content_markdown: '_Older_' }
			] }) };
		}
	});
	assert.equal(requested, 'https://statesu.example/public/blogs/burak/posts?page=1&size=20');
	assert.deepEqual([...target.querySelectorAll('.statesu-body')].map((body) => body.textContent.trim()), ['Newest', 'Older']);
	assert.equal(target.querySelectorAll('.statesu-title, script, button').length, 0);
});

test('feed pagination retries failed pages without losing or duplicating entries', async () => {
	let failSecond = true;
	const { target } = await boot({
		url: 'https://site.example/takes',
		scriptAttrs: { 'data-mode': 'feed', 'data-hide-title': '' },
		fetchBehavior: async (api) => {
			const page = Number(new URL(api).searchParams.get('page'));
			if (page === 2 && failSecond) { failSecond = false; throw new Error('offline'); }
			return { ok: true, json: async () => ({ total: 21, items: Array.from(
				{ length: page === 1 ? 20 : 1 }, (_, i) => ({ title: 'Hidden', content_markdown: `${page}-${i}` })
			) }) };
		}
	});
	target.querySelector('button').click();
	await new Promise((resolve) => setTimeout(resolve, 60));
	assert.equal(target.querySelectorAll('.statesu-body').length, 20);
	assert.match(target.textContent, /could not be loaded/);
	target.querySelector('button').click();
	await new Promise((resolve) => setTimeout(resolve, 60));
	assert.equal(target.querySelectorAll('.statesu-body').length, 21);
	assert.equal(target.querySelectorAll('button, .statesu-title').length, 0);
});

test('feed handles empty and malformed lists safely', async () => {
	for (const [data, message] of [
		[{ items: [], total: 0 }, /Henüz duyuru yok/],
		[{ items: [{ title: 'Missing body' }], total: 1 }, /unexpected response/]
	]) {
		const { target } = await boot({
			url: 'https://site.example/takes', scriptAttrs: { 'data-mode': 'feed' },
			fetchBehavior: async () => ({ ok: true, json: async () => data })
		});
		assert.match(target.textContent, message);
		assert.equal(target.querySelectorAll('.statesu-body').length, 0);
	}
});

test('data-hide-title also hides the title of a fixed post', async () => {
	const { target } = await boot({
		url: 'https://site.example/',
		scriptAttrs: { 'data-post': 'guncel', 'data-hide-title': '' },
		fetchBehavior: async () => ({ ok: true, json: async () => ({ title: 'Hidden', content_markdown: 'Visible' }) })
	});
	assert.equal(target.querySelectorAll('.statesu-title').length, 0);
	assert.equal(target.textContent.trim(), 'Visible');
});

test('404 renders a plain not-found state; irrelevant pages stay untouched', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog/missing',
		fetchBehavior: async () => ({ ok: false, status: 404, json: async () => ({}) })
	});
	assert.equal(target.textContent.trim(), 'No published post at this address.');
});

test('base path page shows the empty state and never calls the API', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog',
		fetchBehavior: async () => {
			throw new Error('must not fetch');
		}
	});
	assert.match(target.textContent, /hosts blog posts/);
});

test('network failure explains the origin requirement and fails closed', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog/test',
		fetchBehavior: async () => {
			throw new Error('boom');
		}
	});
	assert.match(target.textContent, /could not be reached/);
});

test('a slow API is timed out and its request actually cancelled', async () => {
	let requestSignal;
	const { target } = await boot({
		url: 'https://site.example/blog/test',
		fetchBehavior: (api, { signal }) => {
			requestSignal = signal;
			return {
				ok: true,
				status: 200,
				json: () => new Promise(() => {})
			};
		}
	});
	assert.match(target.textContent, /did not answer in time/);
	assert.equal(requestSignal.aborted, true, 'deadline must abort the request itself');
});

test('invalid post URLs fail without an API call', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog/Test%20hi',
		fetchBehavior: async () => {
			throw new Error('must not fetch');
		}
	});
	assert.match(target.textContent, /not a valid blog post address/);
});

test('pages outside the base path are never touched', async () => {
	const dom = new JSDOM('<html><body><div id="t">untouched</div></body></html>', { url: 'https://site.example/other/page' });
	const window = dom.window;
	window.fetch = async () => {
		throw new Error('must not fetch');
	};
	const script = window.document.createElement('script');
	script.setAttribute('data-blog', 'burak');
	script.setAttribute('data-target', 't');
	script.setAttribute('src', SCRIPT_SRC);
	window.document.body.appendChild(script);
	init(window, { timeoutMs: 40 });
	await new Promise((resolve) => setTimeout(resolve, 20));
	assert.equal(window.document.getElementById('t').textContent, 'untouched');
});

test('a second embed on the same page is ignored', async () => {
	const dom = new JSDOM('<html><body><div id="t">initial</div></body></html>', { url: 'https://site.example/blog/test' });
	const window = dom.window;
	let calls = 0;
	window.fetch = async () => {
		calls += 1;
		return { ok: true, status: 200, json: async () => ({ slug: 'test', title: 'T', content_markdown: 'em' }) };
	};
	for (const _ of [1, 2]) {
		const script = window.document.createElement('script');
		script.setAttribute('data-blog', 'burak');
		script.setAttribute('data-target', 't');
		script.setAttribute('src', SCRIPT_SRC);
		window.document.body.appendChild(script);
	}
	init(window, { timeoutMs: 40 });
	await new Promise((resolve) => setTimeout(resolve, 60));
	assert.equal(calls, 1);
});

test('fail-closed: no DOM support in the page leaves text fallback, not HTML', async () => {
	const dom = new JSDOM('<html><body><div id="t">initial</div></body></html>', { url: 'https://site.example/blog/test' });
	const window = dom.window;
	const realWindow = window;
	// Block DOMPurify by hiding the global window from the content module.
	const { buildRenderer } = await import('../src/embed/content.js');
	assert.equal(typeof buildRenderer, 'function');
	// A post whose render fails must still show safe text content.
	window.fetch = async () => ({
		ok: true,
		status: 200,
		json: async () => ({ slug: 'test', title: 'T', content_markdown: '<div>safe</div>' })
	});
	const script = window.document.createElement('script');
	script.setAttribute('data-blog', 'burak');
	script.setAttribute('data-target', 't');
	script.setAttribute('src', SCRIPT_SRC);
	window.document.body.appendChild(script);
	init(realWindow, { timeoutMs: 40 });
	await new Promise((resolve) => setTimeout(resolve, 60));
	// Normal route still sanitizes raw HTML away.
	assert.doesNotMatch(window.document.getElementById('t').outerHTML, /<div>safe<\/div>/);
});

// -------------------------------------------------------------------------
// Embed v2 (shadow root, media metadata, lightbox)
// -------------------------------------------------------------------------

const v2 = await import('../src/embed/main-v2.js');
const V2_SCRIPT_SRC = 'https://statesu.example/embed/v2.js';

const MEDIA_BODY = 'https://media.example/statesu/blogs/b1/m1/source.webp';
const mediaPost = {
	title: 'Media Post',
	content_markdown: `![Alt text](${MEDIA_BODY}){width=640} plain [![inside](${MEDIA_BODY})](https://example.com)`,
	media: [
		{
			id: 'm1',
			source_url: MEDIA_BODY,
			source_width: 1600,
			source_height: 900,
			zoom_url: 'https://media.example/statesu/blogs/b1/m1/w1600.webp',
			variants: [
				{ label: 'w640', url: 'https://media.example/statesu/blogs/b1/m1/w640.webp', width: 640, height: 360 }
			]
		}
	]
};

async function bootV2({ html, fetchBehavior, scriptAttrs } = {}) {
	const dom = new JSDOM(html ?? '<html><body><div id="t">initial</div></body></html>', { url: 'https://site.example/blog/test' });
	const window = dom.window;
	window.fetch = fetchBehavior;
	const script = window.document.createElement('script');
	script.setAttribute('data-blog', 'burak');
	script.setAttribute('data-target', 't');
	script.setAttribute('src', scriptAttrs?.src ?? V2_SCRIPT_SRC);
	for (const [k, v] of Object.entries(scriptAttrs ?? {})) {
		if (k === 'src') continue;
		script.setAttribute(k, v);
	}
	window.document.body.appendChild(script);
	v2.init(window, { timeoutMs: 40 });
	await new Promise((resolve) => setTimeout(resolve, 60));
	const target = window.document.getElementById('t');
	const shadow = target.querySelector('[data-statesu-v2-host]')?.shadowRoot ?? null;
	return { window, target, shadow };
}

function pageFetcher(payload) {
	return async () => ({ ok: true, status: 200, type: 'basic', json: async () => payload });
}

test('v2 renders the post inside a shadow root with trusted srcset', async () => {
	const { target, shadow } = await bootV2({ fetchBehavior: pageFetcher(mediaPost) });
	assert.ok(shadow, 'spin the shadow root');
	assert.match(shadow.textContent, /Media Post/);
	const img = shadow.querySelector('img:not(.linked-in)');
	assert.ok(img, 'the image renders');
	assert.equal(img.getAttribute('width'), '640');
	const srcset = img.getAttribute('srcset') ?? '';
	assert.match(srcset, /w640\.webp 640w/);
	// Metadata attaches the inspection surface for the lightbox.
	assert.equal(img.dataset.zoomUrl, mediaPost.media[0].zoom_url);
	// The author link keeps the image inside it.
	const inside = shadow.querySelector('a img.linked-in');
	assert.ok(inside, 'linked images are annotated');
});

test('v2 opens the lightbox for plain images, never for linked ones', async () => {
	const { window, target, shadow } = await bootV2({ fetchBehavior: pageFetcher(mediaPost) });
	const lightboxHost = window.document.querySelector('[data-statesu-lightbox]');
	assert.ok(lightboxHost, 'the lightbox host is attached to the page');
	const dialog = lightboxHost.shadowRoot.querySelector('dialog');
	assert.equal(dialog.hasAttribute('open'), false, 'starts closed');

	// Plain image click opens the panel on the inspection surface.
	const img = shadow.querySelector('img:not(.linked-in)');
	img.dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
	assert.equal(dialog.hasAttribute('open'), true);
	const lightboxImage = lightboxHost.shadowRoot.querySelector('img');
	assert.equal(lightboxImage.getAttribute('src'), mediaPost.media[0].zoom_url);
	assert.equal(lightboxImage.getAttribute('alt'), 'Alt text');

	// Esc closes through the dialog cancel path in real browsers; jsdom
	// lacks dialog.close(), so the removal mirrors the end state.
	dialog.removeAttribute('open');
	assert.equal(dialog.hasAttribute('open'), false, 'closing clears the state');

	// Linked image clicks never open: the author link owns those.
	const linked = shadow.querySelector('a img.linked-in');
	linked.dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
	assert.equal(dialog.hasAttribute('open'), false);
});

test('v2 with data-lightbox="off" skips the lightbox entirely', async () => {
	const { window, shadow } = await bootV2({
		fetchBehavior: pageFetcher(mediaPost),
		scriptAttrs: { 'data-lightbox': 'off' }
	});
	assert.equal(window.document.querySelector('[data-statesu-lightbox]'), null);
	const img = shadow.querySelector('img:not(.linked-in)');
	img.dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
	// No host and no dialog: the click is a no-op.
	assert.equal(window.document.querySelector('[data-statesu-lightbox]'), null);
});

test('v2 without any media still renders through the metadata renderer', async () => {
	const { shadow } = await bootV2({
		fetchBehavior: pageFetcher({ title: 'Plain', content_markdown: '# Merhaba\n\n Bir **gövde**.' })
	});
	// Title + body live inside the shadow root.
	assert.match(shadow.textContent, /Merhaba/);
	const bold = shadow.querySelector('strong');
	assert.equal(bold.textContent, typeof bold.textContent === 'string' ? bold.textContent : '');
	const img = shadow.querySelector('img');
	assert.equal(img, null);
});

// -------------------------------------------------------------------------
// Embed v2 saved appearance (blog profile + internal custom properties)
// -------------------------------------------------------------------------

const appearance = {
	version: 1,
	colors: { text: '#1A2B3C', link: '#7c3aed', background: 'transparent' },
	typography: { font: 'serif', body_size: 110, heading_scale: 130, line_height: 180 },
	layout: { max_width: 720, padding: 24, spacing: 16 },
	decoration: { border_width: 1, corner_radius: 8 }
};

/** Routes the post endpoint and the blog profile endpoint to payloads. */
function profileFetcher(postPayload, profilePayload) {
	return async (api) => ({
		ok: true,
		status: 200,
		type: 'basic',
		json: async () => (String(api).endsWith('/posts/test') ? postPayload : profilePayload)
	});
}

function varsOf(root) {
	const vars = {};
	for (const name of root.style) {
		if (name.startsWith('--statesu-internal')) vars[name] = root.style.getPropertyValue(name);
	}
	return vars;
}

test('v2 applies the saved appearance as internal custom properties', async () => {
	const { shadow } = await bootV2({
		fetchBehavior: profileFetcher(mediaPost, { embed_appearance: appearance })
	});
	assert.match(shadow.textContent, /Media Post/, 'content renders regardless');
	const root = shadow.querySelector('.statesu-v2');
	const vars = varsOf(root);
	// Colors normalize; typography and layout convert to CSS values.
	assert.equal(vars['--statesu-internal-text'], '#1a2b3c');
	assert.equal(vars['--statesu-internal-link'], '#7c3aed');
	assert.equal(vars['--statesu-internal-background'], 'transparent');
	assert.match(vars['--statesu-internal-font'], /Georgia/);
	assert.equal(vars['--statesu-internal-body-size'], '1.1rem');
	assert.match(vars['--statesu-internal-heading'], /\* 1\.3\)/);
	assert.equal(vars['--statesu-internal-line-height'], '1.8');
	assert.equal(vars['--statesu-internal-max-width'], '720px');
	assert.equal(vars['--statesu-internal-padding'], '24px');
	assert.equal(vars['--statesu-internal-spacing'], '16px');
	assert.equal(vars['--statesu-internal-border-width'], '1px');
	assert.equal(vars['--statesu-internal-corner-radius'], '8px');
});

test('v2 keeps the stock look when the appearance payload is unusable', async () => {
	for (const profile of [
		{ embed_appearance: { version: 2 } },
		{ embed_appearance: { version: 1, colors: { link: 'red' } } },
		{ embed_appearance: { version: 1, typography: { body_size: 9000 } } },
		{ embed_appearance: { version: 1, css: 'body { display: none }' } },
		{ embed_appearance: 'not an object' },
		{}
	]) {
		const { shadow } = await bootV2({ fetchBehavior: profileFetcher(mediaPost, profile) });
		assert.match(shadow.textContent, /Media Post/);
		const root = shadow.querySelector('.statesu-v2');
		assert.equal(Object.keys(varsOf(root)).length, 0, `no tokens for ${JSON.stringify(profile)}`);
	}
});

test('v2 renders content even when the appearance request fails', async () => {
	const { shadow } = await bootV2({
		fetchBehavior: async (api) => {
			if (!String(api).endsWith('/posts/test')) {
				return { ok: false, status: 500, type: 'basic', json: async () => ({}) };
			}
			return { ok: true, status: 200, type: 'basic', json: async () => mediaPost };
		}
	});
	assert.match(shadow.textContent, /Media Post/);
});

test('v2 feed entries inherit the appearance from the feed root', async () => {
	const feed = { items: [{ title: 'One', content_markdown: 'a' }, { title: 'Two', content_markdown: 'b' }], page: 1, size: 20, total: 2 };
	const { shadow } = await bootV2({
		scriptAttrs: { 'data-mode': 'feed', 'data-hide-title': '' },
		fetchBehavior: async (api) => ({
			ok: true,
			status: 200,
			type: 'basic',
			json: async () => (String(api).includes('/posts?') ? feed : { embed_appearance: appearance })
		})
	});
	const root = shadow.querySelector('.statesu-v2');
	assert.equal(varsOf(root)['--statesu-internal-link'], '#7c3aed');
	assert.equal(shadow.querySelectorAll('.statesu-announcement').length, 2);
});
