import { mkdir, mkdtemp, rename, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createBlogPages } from './customer-blog.js';

const site = 'https://statesu.com';
const basePath = '/demo/blog';
const samplePost = {
	slug: 'welcome-to-the-writing-desk',
	title: 'Welcome to the Writing Desk',
	content_markdown: `This sample post was written in Markdown and rendered to HTML before you opened it. Disable JavaScript or view the page source: this paragraph, the post title, and its canonical URL are already in the response.

## A place to keep short notes

A blog doesn't need a new site. Keep your homepage as it is and give your field notes, release updates, or weekend projects a home under a path like \`/blog/\`. From there, link to each post so readers can bookmark or share it directly.

Want to try it on your own site? Follow the [static export walkthrough](/docs/integration#static-export).`
};

function demoPages(blog) {
	return createBlogPages({
		site,
		blog: blog || 'writing-desk',
		basePath,
		title: 'Statesu Demo Blog',
		description: blog
			? 'These posts were exported from the Statesu public API at build time. You can read them without JavaScript.'
			: 'This demo uses sample Statesu API data to show how a Markdown blog can be exported as HTML. You can read it without JavaScript.',
		stylesheet: '/demo-blog.css',
		...(blog ? {} : {
			fetcher: async (input) => {
				const endpoint = `${site}/public/blogs/writing-desk/posts`;
				if (input === `${endpoint}?page=1&size=100`) {
					return Response.json({ items: [samplePost], page: 1, size: 100, total: 1 });
				}
				if (input === `${endpoint}/${samplePost.slug}`) return Response.json(samplePost);
				return new Response(null, { status: 404 });
			}
		})
	});
}

// Vite's dev server has no static export; serve the same sample through the
// renderer instead of requiring a production build or a running API server.
export function demoBlogDev() {
	return {
		name: 'statesu-demo-blog',
		configureServer(server) {
			const pages = demoPages();
			server.middlewares.use((request, response, next) => {
				const path = request.url.split('?', 1)[0];
				if (path !== basePath && !path.startsWith(`${basePath}/`)) return next();
				pages.respond(request.url, request.method, true).then(async (result) => {
					if (!result) return next();
					const body = Buffer.from(await result.arrayBuffer());
					response.writeHead(result.status, Object.fromEntries(result.headers)).end(body);
				}).catch(next);
			});
		}
	};
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
	const pages = demoPages(process.env.STATESU_DEMO_BLOG);
	// Generate into a fresh directory and replace only the previous demo export.
	await mkdir('build/demo', { recursive: true });
	const temporary = await mkdtemp('build/.demo-');
	try {
		const output = join(temporary, 'output');
		await pages.exportTo(output);
		await rm('build/demo/blog', { recursive: true, force: true });
		await rename(join(output, 'demo', 'blog'), 'build/demo/blog');
	} finally {
		await rm(temporary, { recursive: true, force: true });
	}
}
