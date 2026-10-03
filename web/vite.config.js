import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Proxy API requests during local documentation development.
const apiTarget = process.env.STATESU_API ?? 'http://localhost:8080';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// The homepage is prerendered as static documentation.
			adapter: adapter()
		})
	],
	server: {
		proxy: {
			'/auth': apiTarget,
			'/state': apiTarget
		}
	}
});
