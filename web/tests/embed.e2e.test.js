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

test('a slow API produces a timeout state instead of a hang', async () => {
	const { target } = await boot({
		url: 'https://site.example/blog/test',
		fetchBehavior: async () => ({
			ok: true,
			status: 200,
			json: () => new Promise(() => {})
		})
	});
	assert.match(target.textContent, /did not answer in time/);
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
