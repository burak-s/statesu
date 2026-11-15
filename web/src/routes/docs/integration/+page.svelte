<script>
	import Terminal from '#lib/components/Terminal.svelte';

	const overrides = `/* The website always wins over the saved appearance. */
#statesu-post {
  --statesu-text-color: #171717;
  --statesu-background-color: #ffffff;
  --statesu-link-color: #7c3aed;
  --statesu-muted-color: #6e6e6e;
  --statesu-border-color: #dddddd;
  --statesu-font-family: Georgia, serif;
  --statesu-body-size: 1.05rem;
  --statesu-heading-size: 1.7em;
  --statesu-line-height: 1.75;
  --statesu-max-width: 720px;
  --statesu-padding: 24px;
  --statesu-spacing: 16px;
  --statesu-border-width: 1px;
  --statesu-corner-radius: 12px;
}`;
</script>

<svelte:head>
	<title>Web integration — Statesu</title>
	<meta name="description" content="Display Statesu posts on your website with an embed script or public JSON. Configure hosting, CORS and safe Markdown rendering." />
	<link rel="canonical" href="https://statesu.com/docs/integration" />
</svelte:head>

<section class="intro">
	<p class="eyebrow">Web integration</p>
	<h1>Your posts.<br />Your frontend.</h1>
	<p class="lead">Use one embed script to display posts, or fetch public JSON and render it yourself. No token belongs in your website code.</p>
</section>

<section>
	<h2>1. Allow your website</h2>
	<p>In the <a href="/dashboard">dashboard</a>, open your blog's settings and add your website's origin, such as <code>https://example.com</code>. You can also add <code>http://localhost:5173</code> for development.</p>
	<p>Origins are exact scheme, host and optional port — no paths, queries or credentials. Each blog supports up to 32 origins.</p>
</section>

<section>
	<h2>2. Add the embed</h2>
	<p>Copy your blog's snippet from dashboard Settings into your site's post template. Replace the blog slug below with your own.</p>
	<Terminal code={'<div id="statesu-post"></div>\n<script defer\n  src="https://statesu.com/embed/v2.js"\n  data-blog="writing-desk"\n  data-base-path="/blog"\n  data-target="statesu-post"></script>'} language="html" />
	<p><code>/blog/hello-world</code> displays the published post with slug <code>hello-world</code>. Future posts work the same way; no per-post code changes. Use one embed per page.</p>
	<p>For a fixed announcement on your homepage, add <code>data-post="guncel"</code> to the script. This selects that published post regardless of the page URL; omit it to use URL-based routing.</p>
	<p>To show only the most recently published announcement, use <code>data-mode="latest"</code> instead. Add <code>data-hide-title</code> to display just its Markdown body, without the post title. Latest mode ignores the page URL and <code>data-post</code>; it shows nothing when there are no published posts.</p>
	<h3>Media and the v2 embed</h3>
	<p>Add media images in the dashboard or <a href="/docs/cli">CLI</a>; each upload renders one standard Markdown image tag like <code>![alt](https://media.example.com/statesu/blogs/blogs/…/source.webp)</code>. Served from your own public media domain, the URL is permanent and moves with your Markdown.</p>
	<p><code>/embed/v2.js</code> keeps v1 semantics and adds visual media handling inside its own shadow DOM: host page styles don't clash, responsive variants load by viewport, images can carry width pins, and a <code>data-lightbox="off"</code> attribute disables click-to-zoom. Linked images are never intercepted; the author's destination always wins. v2 is recommended for new setups — it is also the embed that applies your saved appearance.</p>
	<Terminal code={'<div id="statesu-post"></div>\n<script defer\n  src="https://statesu.com/embed/v2.js"\n  data-blog="writing-desk"\n  data-base-path="/blog"\n  data-target="statesu-post"></script>'} language="html" />
	<p>For an announcement archive, use <code>data-mode="feed"</code> with <code>data-hide-title</code>. All published entries appear newest first, with a load-more button after 20 entries. Feed mode also ignores the page URL and <code>data-post</code>.</p>
</section>

<section>
	<h2>Style your embeds once</h2>
	<p>In the dashboard, open your blog's settings and use <strong>Embed appearance</strong>: pick a preset, adjust colors, typography, layout and decoration against a live preview, and save. The saved appearance is stored per blog and applied by every <code>/embed/v2.js</code> installation automatically — visitors' snippets never change, and your next save shows up on their next page load.</p>
	<p>The controls are typed style tokens, not free-form CSS: colors accept <code>#rgb</code>, <code>#rrggbb</code> or (for the background) <code>transparent</code>; sizes are percentages of the built-in value; layout and decoration are pixels with documented bounds. Anything unset keeps the built-in look, and values that fail validation are rejected at save time.</p>
	<p>Font families are inherited from the host website’s CSS, or use the browser default when none is set. The embed never downloads fonts and saved appearances cannot choose a font family. To override the inherited family explicitly, set <code>--statesu-font-family</code> on the embed target.</p>
	<p>Choose <strong>Fields</strong> for grouped controls with individual resets, or <strong>JSON</strong> for direct editing of the same draft. JSON mode lists every supported field with its default already filled in, preserving your overrides. Change a value to customize it; type the default back to restore it. A few defaults cannot be a fixed value because they follow your website or the browser: keywords name them. <code>inherit</code>, <code>revert</code>, <code>currentcolor</code> and <code>transparent</code> keep the built-in or website-inherited styling for colors, <code>revert</code> does the same for sizes and <code>none</code> removes the width limit. Keywords are dropped on save, so a document of only defaults is the same as clearing; colors also accept <code>rgb()</code>/<code>rgba()</code> and 8-digit hex. JSON edits are validated before saving; invalid edits keep the last valid preview and cannot be switched into Fields until corrected or discarded. Presets ask before replacing unsaved work. Use <strong>Load my posts</strong> to preview your own published or draft content, and the Edit styles / See preview tabs on small screens.</p>
	<h3>Saved styles</h3>
	<p>Arrange several complete stylings and switch between them: name the styling currently shown in the editor with <strong>Save current as style</strong>, and apply or delete snapshots from the list. Applying a style replaces the blog's saved appearance for every embed; deleting removes only the snapshot. Styles are per blog, names are unique ignoring case, and each blog can keep up to 50. The API stores them under <code>POST</code>/<code>GET /blogs/&#123;blogID&#125;/appearance/variants</code> (and <code>DELETE …/variants/&#123;variantID&#125;</code>), and <code>stscli blog appearance save|list|apply|delete</code> drives the same endpoints.</p>
	<h3>Markdown element styles</h3>
	<p>The dashboard, CLI and API share one version-1 appearance document. All groups and tokens below are optional. Saving replaces the whole document; omitted tokens return to defaults. Markdown content is never rewritten.</p>
	<ul>
		<li><code>colors</code>: <code>text</code>, <code>background</code>, <code>link</code>, <code>muted</code>, <code>border</code> — colors in hex (3, 4, 6 or 8 digits), <code>rgb()</code> or <code>rgba()</code>; the global background also accepts <code>transparent</code>. Defaults are keywords: inherited text, native link colors, the text color for muted and a subtle border.</li>
		<li><code>typography</code>: <code>body_size</code> (75–200%), <code>heading_scale</code> (100–225%), <code>line_height</code> (100–250%).</li>
		<li><code>layout</code>: <code>max_width</code> (320–1200px), <code>padding</code> and <code>spacing</code> (0–96px).</li>
		<li><code>decoration</code>: <code>border_width</code> (0–8px), <code>corner_radius</code> (0–32px).</li>
		<li><code>markdown</code> spacing (0–96px): <code>paragraph_spacing</code>, <code>heading_spacing</code>, <code>list_indent</code>, <code>list_spacing</code>, <code>code_padding</code>, <code>quote_padding</code>, <code>table_cell_padding</code>.</li>
		<li><code>markdown</code> colors (hex): <code>code_color</code>, <code>code_background</code>, <code>inline_code_color</code>, <code>inline_code_background</code>, <code>quote_color</code>, <code>quote_background</code>, <code>quote_border_color</code>, <code>table_header_background</code>, <code>table_border_color</code>, <code>rule_color</code>.</li>
		<li><code>markdown</code> decoration: <code>quote_border_width</code> and <code>rule_width</code> (0–8px), <code>image_radius</code> (0–32px).</li>
		<li>Individual headings: <code>h1_size</code> through <code>h6_size</code> (12–96px), <code>h1_weight</code> through <code>h6_weight</code> (100–900), and <code>h1_spacing</code> through <code>h6_spacing</code> (0–96px above and below). Individual sizes replace the global heading scale for that level; these style Markdown headings, not the separate post title.</li>
		<li>Links: <code>link_decoration</code> and <code>link_hover_decoration</code> accept <code>underline</code> or <code>none</code>; <code>link_hover_color</code> accepts hex. Hover styles also apply to keyboard focus.</li>
		<li>Code blocks: <code>code_size</code> (8–48px), <code>code_border_color</code> (hex), <code>code_border_width</code> (0–8px), <code>code_radius</code> (0–32px).</li>
		<li>Inline code: <code>inline_code_size</code> (8–48px), <code>inline_code_padding</code> (0–96px), <code>inline_code_radius</code> (0–32px), <code>inline_code_border_color</code> (hex), <code>inline_code_border_width</code> (0–8px).</li>
		<li>Tables: <code>table_header_color</code> and <code>table_stripe_background</code> (hex). The stripe color applies to every second body row; reset it to remove stripes.</li>
		<li>Images: <code>image_width</code> (1–100% of the content width), <code>image_spacing</code> (0–96px above and below), <code>image_align</code> (<code>left</code>, <code>center</code>, <code>right</code>). Markdown width pins take precedence; linked images retain their inline placement.</li>
	</ul>
	<p>Dimensions must be integers. Code blocks and tables scroll horizontally when needed. Element-specific spacing overrides global spacing. For Markdown tokens, replace underscores with hyphens and prefix with <code>--statesu-</code> to override them on your website; for example, <code>--statesu-code-background</code>. The exception is image alignment, which maps to <code>--statesu-image-margin-left</code> and <code>--statesu-image-margin-right</code> (use <code>0</code> / <code>auto</code>).</p>
	<h3>Developer overrides</h3>
	<p>A website can still steer individual tokens with CSS custom properties on the embed target, without touching the shadow tree:</p>
	<Terminal code={overrides} language="css" />
	<p>Precedence, strongest first: the website's CSS → the blog's saved appearance → the built-in defaults. Every token has a variable, so an embed can blend into any site while still following the owner's branding.</p>
	<h3>v1 embeds</h3>
	<p><code>/embed/v1.js</code> keeps serving pages unchanged and does not apply the saved appearance — its look is styled by the host page directly. Switch the script URL to <code>/embed/v2.js</code> to receive saved styling; the rest of the snippet stays the same.</p>
</section>

<section>
	<h2>3. Route post URLs to your template</h2>
	<p>Your host must serve the same HTML template for every post URL. Choose the example for your host:</p>
	<Terminal code={'# Netlify _redirects\n/blog/*      /blog.html   200\n\n# vercel.json\n{ "rewrites": [{ "source": "/blog/:path*", "destination": "/blog.html" }] }\n\n# Nginx (inside your site server block)\nlocation /blog/ {\n    try_files $uri /blog.html;\n}'} language="text" />
	<p>Open a published deep link directly and refresh it to check the setup. The base path shows an empty state; draft, withdrawn and deleted posts show a not-found state.</p>
	<p>For root-level URLs like <code>/hello-world</code>, set <code>data-base-path="/"</code>. Rewrite only unknown single-segment paths so existing pages keep working.</p>
</section>

<section>
	<h2>Prefer your own renderer?</h2>
	<p>Fetch published posts as JSON without authentication. Server-side fetching does not need a browser origin. See the <a href="/docs/api">API guide</a> for endpoints and pagination.</p>
	<Terminal code={'const response = await fetch(\n  \'https://statesu.com/public/blogs/writing-desk/posts/hello-world\'\n);\nif (!response.ok) throw new Error(\'Could not fetch post\');\nconst post = await response.json();\ndocument.querySelector(\'#title\').textContent = post.title;\n// Render post.content_markdown with a safe Markdown renderer.'} language="javascript" />
</section>

<section>
	<h2>Security and rendering</h2>
	<ul>
		<li>Published posts are public. CORS limits browser access, not server-side requests. Never publish secrets or expose a personal access token.</li>
		<li>Markdown is untrusted content. Disable raw HTML or sanitize rendered output; use <code>textContent</code> for titles. The embed uses marked and DOMPurify with a strict allowlist; media images are restricted to http(S) source origins, so hostile data: or javascript: URLs are ignored.</li>
		<li>The embed renders client-side. For post content in initial HTML and real HTTP 404 responses, render server-side instead.</li>
		<li>If your site has a Content-Security-Policy, allow your Statesu origin in both <code>script-src</code> and <code>connect-src</code>.</li>
	</ul>
</section>
