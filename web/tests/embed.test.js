import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';

// URL resolution and API building are DOM-free.
import { parseBasePath, resolveSlug, SLUG_PATTERN } from '../src/embed/url.js';
import { parseApiOrigin, apiUrl, blogApiUrl } from '../src/embed/api.js';
import { parseAppearance, appearanceVars, applyAppearance, RANGES } from '../src/embed/appearance.js';

// DOMPurify needs a window before its factory is instantiated, so the
// content module is imported once a jsdom window is installed as global.
const dom = new JSDOM('<!doctype html><html><body></body></html>', { url: 'https://site.example/blog/test' });
globalThis.window = dom.window;
const { buildRenderer } = await import('../src/embed/content.js');
const render = buildRenderer(globalThis.window);
assert.ok(render, 'renderer should be available under jsdom');

const POST_URL = 'https://site.example/blog/test';
const POST_URL_IN_HEAD = 'https://site.example/blog';

test('slug pattern mirrors backend rules', () => {
	assert.ok(SLUG_PATTERN.test('test'));
	assert.ok(SLUG_PATTERN.test('deneme-2'));
	assert.ok(!SLUG_PATTERN.test('Test'));
	assert.ok(!SLUG_PATTERN.test('-a'));
	assert.ok(!SLUG_PATTERN.test('a-'));
	assert.ok(!SLUG_PATTERN.test('a_b'));
	assert.ok(!SLUG_PATTERN.test(''));
});

test('basePath normalization and validation', () => {
	assert.equal(parseBasePath(undefined), '/blog');
	assert.equal(parseBasePath(''), '/blog');
	assert.equal(parseBasePath('/blog'), '/blog');
	assert.equal(parseBasePath('/blog/'), '/blog');
	assert.equal(parseBasePath('/notes'), '/notes');
	assert.equal(parseBasePath('/'), '/');
	assert.throws(() => parseBasePath('blog'), Error);
	assert.throws(() => parseBasePath('/blog/posts'), Error);
	assert.throws(() => parseBasePath('/Blog'), Error);
});

test('resolveSlug: posts, empty state, skip and invalid URLs', () => {
	assert.deepEqual(resolveSlug('https://site.example/blog/test', '/blog'), { kind: 'post', slug: 'test' });
	assert.deepEqual(resolveSlug('https://site.example/blog/deneme?v=1#top', '/blog'), { kind: 'post', slug: 'deneme' });
	assert.deepEqual(resolveSlug('https://site.example/blog/test/', '/blog'), { kind: 'post', slug: 'test' });
	assert.deepEqual(resolveSlug('https://site.example/blog', '/blog'), { kind: 'empty' });
	assert.deepEqual(resolveSlug('https://site.example/blog/', '/blog'), { kind: 'empty' });
	// Segment-boundary matching: `/blogger` is not under `/blog`.
	assert.deepEqual(resolveSlug('https://site.example/blogger', '/blog'), { kind: 'skip' });
	assert.deepEqual(resolveSlug('https://site.example/other/post', '/blog'), { kind: 'skip' });
	// Multi-segment and encoded-slash URLs never reach the API.
	assert.equal(resolveSlug('https://site.example/blog/a/b', '/blog').kind, 'invalid');
	assert.equal(resolveSlug('https://site.example/blog/a%2Fb', '/blog').kind, 'invalid');
	assert.equal(resolveSlug('https://site.example/blog/%E2%28', '/blog').kind, 'invalid');
	assert.equal(resolveSlug('https://site.example/blog/Test', '/blog').kind, 'invalid');
});

test('resolveSlug: root base path', () => {
	assert.deepEqual(resolveSlug('https://site.example/', '/'), { kind: 'empty' });
	assert.deepEqual(resolveSlug('https://site.example/test', '/'), { kind: 'post', slug: 'test' });
	assert.equal(resolveSlug('https://site.example/test/extra', '/').kind, 'invalid');
});

test('api URL building encodes slug segments', () => {
	assert.equal(apiUrl('https://api.example', 'burak', 'test'), 'https://api.example/public/blogs/burak/posts/test');
	assert.equal(apiUrl('https://api.example', 'burak', 'a/b'), 'https://api.example/public/blogs/burak/posts/a%2Fb');
	assert.equal(blogApiUrl('https://api.example', 'burak'), 'https://api.example/public/blogs/burak');
});

// The parser is the shared contract with the backend validator
// (src/blog/appearance.rs); the dashboard editor and the embed both lean on
// it, so it must accept exactly what the server accepts.
test('appearance parser ignores retired fonts and keeps neighbouring tokens', () => {
	const parsed = parseAppearance({ version: 1, typography: { font: 'inherit', body_size: 110 } });
	assert.deepEqual(parsed.typography, { body_size: 110 });
	const vars = appearanceVars(parsed);
	assert.equal(vars['--statesu-internal-font'], undefined);
	assert.equal(vars['--statesu-internal-body-size'], '1.1rem');
});

test('retired font settings never set a font family or download fonts', () => {
	const doc = new JSDOM('<!doctype html><html><head></head><body></body></html>').window.document;
	const host = doc.createElement('div');
	doc.body.appendChild(host);
	const root = doc.createElement('article');
	host.attachShadow({ mode: 'open' }).appendChild(root);
	applyAppearance(root, parseAppearance({ version: 1, typography: { font: 'serif' } }));
	assert.equal(doc.head.querySelectorAll('link').length, 0);
	const appearance = parseAppearance({ version: 1, typography: { google_font: 'Noto Sans JP', font: 'serif' } });
	applyAppearance(root, appearance);
	applyAppearance(doc.createElement('article'), appearance);
	assert.equal(doc.head.querySelectorAll('link').length, 0);
	assert.equal(root.style.getPropertyValue('--statesu-internal-font'), '');
	applyAppearance(root, parseAppearance({ version: 1, typography: { google_font: 'Inter&family=Other' } }));
	assert.equal(doc.head.querySelectorAll('link').length, 0);
});

test('appearance parser mirrors backend bounds', () => {
	assert.deepEqual(RANGES, {
		body_size: [75, 200],
		heading_scale: [100, 225],
		line_height: [100, 250],
		max_width: [320, 1200],
		padding: [0, 96],
		spacing: [0, 96],
		border_width: [0, 8],
		corner_radius: [0, 32]
	});
	// Invalid fields reject their whole group, not the document; empty
	// groups apply no tokens at all.
	assert.equal(parseAppearance({ version: 1, layout: { padding: 97 } }).layout, null);
	assert.equal(parseAppearance({ version: 1, typography: { unknown: 'comic sans' } }).typography, null);
	assert.equal(parseAppearance({ version: 1, colors: { link: '#ff00' } }).colors, null);
	assert.deepEqual(appearanceVars(parseAppearance({ version: 1, layout: { padding: 97 } })), {});
	const ok = parseAppearance({ version: 1, colors: { link: '#ABC' } });
	assert.equal(ok.colors.link, '#aabbcc');
});

test('api origin policy: https always, http only for local hosts', () => {
	assert.deepEqual(parseApiOrigin('https://statesu.example/embed/v1.js?x'), { ok: true, origin: 'https://statesu.example' });
	assert.deepEqual(parseApiOrigin('http://localhost:8080/embed/v1.js'), { ok: true, origin: 'http://localhost:8080' });
	assert.deepEqual(parseApiOrigin('http://127.0.0.1:8080/embed/v1.js'), { ok: true, origin: 'http://127.0.0.1:8080' });
	assert.deepEqual(parseApiOrigin('http://[::1]:8080/embed/v1.js'), { ok: true, origin: 'http://[::1]:8080' });
	assert.equal(parseApiOrigin('http://site.example/embed/v1.js').ok, false, 'plain http on a public host is refused');
	assert.equal(parseApiOrigin('not-a-url').ok, false);
});

function sanitized(markdown) {
	return render(markdown);
}

function elementCount(html, selector) {
	return new JSDOM(`<body>${html}</body>`).window.document.querySelectorAll(selector).length;
}

test('normal markdown survives sanitization', () => {
	const html = sanitized('# Heading\n\n**bold** _it_ `code`\n\n- one\n- two\n\n[home](https://site.example/)');
	assert.match(html, /<h1>Heading<\/h1>/);
	assert.match(html, /<strong>bold<\/strong>/);
	assert.match(html, /<em>it<\/em>/);
	assert.match(html, /<code>code<\/code>/);
	assert.match(html, /<li>one<\/li>/);
	assert.match(html, /<a /);
});

test('raw HTML is dropped entirely', () => {
	const html = sanitized('text <div class=x>block</div> more <em>raw</em>');
	assert.equal(elementCount(html, 'div'), 0);
	assert.ok(!html.includes('class=x'), 'raw markup must not become markup');
	assert.equal(elementCount(html, 'script'), 0);
	assert.equal(elementCount(html, 'iframe'), 0);
	assert.ok(!html.includes('<em>'), 'inline raw HTML is dropped, not rendered');
	assert.ok(html.includes('raw'), 'the words around raw markup stay as text');
	// Markdown-native emphasis still renders normally.
	assert.match(sanitized('*emphasis*'), /<em>emphasis<\/em>/);
});

test('event handlers, ids and data attributes are removed', () => {
	const html = sanitized('<img src=x onerror=alert(1)> test');
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	assert.equal(doc.querySelectorAll('img').length, 0, 'images are disabled in v1');
	for (const node of doc.querySelectorAll('*')) {
		for (const attribute of node.getAttributeNames()) {
			assert.ok(!attribute.startsWith('on'), `event handler leaked: ${attribute}`);
			assert.ok(!['id', 'name', 'class', 'style', 'srcset'].includes(attribute), `attribute not allowlisted: ${attribute}`);
			assert.ok(!attribute.startsWith('data-'), 'data attributes are disabled');
		}
	}
});

test('markdown images and data URIs are removed', () => {
	const html = sanitized('![tracker](https://evil.example/1px.png) ![payload](data:image/svg+xml,<svg onload=alert(1)>)');
	assert.equal(elementCount(html, 'img'), 0);
	assert.ok(!html.includes('evil.example'));
});

function link(a) {
	const document = new JSDOM(`<body>${a}</body>`).window.document;
	return document.querySelector('a');
}

test('links: only http(s) and relative survive; target/rel are forced', () => {
	const good = link(sanitized('[home](/about)'));
	assert.ok(good, 'relative link kept');
	assert.equal(good.getAttribute('href'), '/about');
	assert.equal(good.getAttribute('target'), '_blank');
	assert.equal(good.getAttribute('rel'), 'noopener noreferrer');

	const httpsLink = link(sanitized('[x](https://site.example/a?b=c)'));
	assert.equal(httpsLink.getAttribute('href'), 'https://site.example/a?b=c');
	assert.equal(httpsLink.getAttribute('target'), '_blank');

	assert.equal(link(sanitized('[bad](javascript:alert(1))')), null);
	assert.equal(link(sanitized('[bad](data:text/html,hi)')), null);
	assert.equal(link(sanitized('[bad](vbscript:msgbox)')), null);
	assert.equal(link(sanitized('[bad](jAvAsCrIpT:alert(1))')), null);
	assert.equal(elementCount(sanitized('[bad](javascript:alert(1))'), 'a'), 0);
});

test('unsafe links are demoted to text, the words stay', () => {
	const doc = new JSDOM(`<body>${sanitized('[Contact us](mailto:help@example.com)')}</body>`).window.document;
	assert.equal(doc.querySelector('a'), null, 'unsafe anchors are removed');
	assert.match(doc.body.textContent, /Contact us/, 'the link text survives');
	assert.equal(link(sanitized('[ftp](ftp://files.example/x)')), null);
});

test('svg, math, style, forms and subscript tags are forbidden', () => {
	const html = sanitized('<svg><script>alert(1)</script></svg>\n<style>a{}</style>\n<form action=x><input onfocus=alert(1)></form>\n<math href="javascript:alert(1)">CLICK</math>');
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	for (const tag of ['svg', 'script', 'style', 'form', 'input', 'math']) {
		assert.equal(doc.querySelectorAll(tag).length, 0, `${tag} must not survive`);
	}
});

test('html entities are not re-parsed as markup', () => {
	const html = sanitized('&lt;img src=x onerror=alert(1)&gt;');
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	assert.equal(doc.querySelectorAll('img').length, 0);
	assert.ok(doc.body.textContent.includes('<img src=x onerror=alert(1)>') || html.includes('&lt;img'));
});

test('fail-closed: renderer availability requires a sanitizing DOM', () => {
	assert.equal(buildRenderer(null), null);
	// A deliberately broken document must yield null — never a renderer that
	// passes raw HTML through. DOMPurify builds its template through
	// DOMImplementation.createHTMLDocument; removing it makes the window
	// genuinely unsupported instead of passing a fake object.
	const dom2 = new JSDOM('<!doctype html><html><body></body></html>', {
		url: 'https://site.example/blog/x'
	});
	dom2.window.DOMImplementation.prototype.createHTMLDocument = undefined;
	assert.equal(buildRenderer(dom2.window), null);
	// A bare non-DOM object stays fail-closed as well.
	const unsupported = { location: { href: 'https://site.example/x' }, document: {} };
	assert.equal(buildRenderer(unsupported), null);
});

test('DOMPurify always strips dangerous output even for pathological input', () => {
	for (const payload of [
		'<a href="javascript:alert(1)">x</a>',
		'<a href="java&#115;cript:alert(1)">x</a>',
		'<a href=" &#14; javascript:alert(1)">x</a>',
		'<p><a href="javascript&colon;alert(1)">x</a>word',
		'<img src="x" onerror="parent.location=\'evil\'">',
		'<blockquote onfocusin="alert(1)" onblur="alert(1)">x</blockquote>',
		'<pre><code>drop="x" id=body formaction=javascript:alert(1)</code></pre>'
	]) {
		const html = sanitized(`text\n\n${payload}\n\nmore`);
		const doc = new JSDOM(`<body>${html}</body>`).window.document;
		for (const node of doc.querySelectorAll('*')) {
			for (const attribute of node.getAttributeNames()) {
				assert.ok(
					['href', 'title', 'target', 'rel'].includes(attribute),
					`unexpected attribute ${attribute} on ${node.nodeName}`
				);
				if (attribute === 'href') {
					const href = node.getAttribute('href');
					const protocol = href ? new URL(href, 'https://site.example/').protocol : '';
					assert.ok(
						['http:', 'https:'].includes(protocol),
						`unsafe href survived: ${href}`
					);
				}
			}
		}
		assert.equal(doc.querySelector('script'), null, 'script must never appear');
	}
});

// -------------------------------------------------------------------------
// Media renderer (embed v2 + dashboard preview shared module)
// -------------------------------------------------------------------------

const { renderImageMarkup, buildMediaRenderer } = await import('../src/embed/media-renderer.js');
// Ad-hoc metadata entry keyed by absolute URL, matching public metadata.
const MEDIA_META = new Map([
	['https://media.example/statesu/blogs/bb/media/source.webp', {
		id: 'm1',
		source_url: 'https://media.example/statesu/blogs/bb/media/source.webp',
		source_width: 1600,
		source_height: 900,
		variants: [
			{ label: 'w640', url: 'https://media.example/statesu/blogs/bb/media/w640.webp', width: 640, height: 360 },
			{ label: 'w1280', url: 'https://media.example/statesu/blogs/bb/media/w1280.webp', width: 1280, height: 720 }
		]
	}]
]);

const mediaRender = buildMediaRenderer(globalThis.window, MEDIA_META);
assert.ok(mediaRender, 'media renderer should be available under jsdom');

test('standard images render with alt and lazy loading', () => {
	const html = mediaRender(`![Sınıf fotoğrafı](https://media.example/statesu/blogs/bb/media/source.webp)`);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	const img = doc.querySelector('img');
	assert.ok(img, 'standard image expected');
	assert.equal(img.getAttribute('alt'), 'Sınıf fotoğrafı');
	assert.equal(img.getAttribute('loading'), 'lazy');
	assert.equal(img.getAttribute('src'), 'https://media.example/statesu/blogs/bb/media/source.webp');
	// Known metadata builds the trusted srcset for its variants.
	const srcset = img.getAttribute('srcset') ?? '';
	assert.match(srcset, /w640\.webp 640w/);
	assert.match(srcset, /w1280\.webp 1280w/);
});

test('width pins stay bounded and update the chosen size metadata', () => {
	const html = mediaRender(`![retina media](https://media.example/statesu/blogs/bb/media/source.webp){width=640}`);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	const img = doc.querySelector('img');
	assert.equal(img.getAttribute('width'), '640');
	assert.equal(img.getAttribute('sizes'), '(max-width: 640px) 100vw, min(640px, 100vw)');
	// Metadata builds the trusted srcset.
	const srcset = img.getAttribute('srcset') ?? '';
	assert.match(srcset, /w640\.webp 640w/);
	assert.match(srcset, /w1280\.webp 1280w/);
});

test('the reserved width suffix is parsed only when shaped form', () => {
	const html = mediaRender(
		`![alt](https://media.example/statesu/blogs/bb/media/source.webp){width=0} and ![plain](https://media.example/statesu/blogs/bb/media/source.webp)`
	);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	const imgs = [...doc.querySelectorAll('img')];
	assert.equal(imgs.length, 2);
	// {width=0} violates the pin: the metadata source width is used
	// instead, and the suffix text itself never leaks as content.
	assert.equal(imgs[0].getAttribute('width'), '1600');
	assert.equal(imgs[1].getAttribute('width'), '1600');
	assert.equal(doc.body.textContent.trim(), 'and');
});

test('javascript: urls are stripped from images and links', () => {
	const html = mediaRender(
		`[merhaba](javascript:alert(1)) and ![x](javascript:alert(2))`
	);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	assert.equal(doc.querySelector('img'), null);
	assert.equal(doc.querySelector('a'), null);
	assert.match(doc.body.textContent, /merhaba/);
	assert.match(doc.body.textContent, /x/);
});

test('raw html still never renders', () => {
	const html = mediaRender(`![alt](https://media.example/statesu/blogs/bb/media/source.webp) <b>bold</b> <img src="x" onerror="alert(1)">`);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	assert.equal(doc.querySelector('b'), null, 'raw html must not render');
	// The tokenizer-parsed image stays, the stray img is dropped.
	assert.equal(doc.querySelectorAll('img').length, 1);
	assert.equal(doc.querySelectorAll('img')[0].getAttribute('src'), 'https://media.example/statesu/blogs/bb/media/source.webp');
});

test('linked images keep the link, lightbox must fight for it', () => {
	const html = mediaRender(`[![alt](https://media.example/statesu/blogs/bb/media/source.webp)](https://example.com)`);
	const doc = new JSDOM(`<body>${html}</body>`).window.document;
	const anchor = doc.querySelector('a');
	assert.ok(anchor, 'the author link must survive');
	assert.equal(anchor.getAttribute('href'), 'https://example.com');
	assert.equal(anchor.getAttribute('target'), '_blank');
	assert.equal(anchor.getAttribute('rel'), 'noopener noreferrer');
	assert.ok(anchor.querySelector('img'), 'the image rides inside the link');
});

test('renderImageMarkup keeps plain words when hrefs are unsafe', () => {
	const output = renderImageMarkup(globalThis.window, MEDIA_META, {
		text: 'kelimeler kalsın',
		href: 'javascript:alert(9)',
		title: 'hiçbir şey',
		width: undefined
	});
	assert.equal(output, 'kelimeler kalsın');
});
