import { defineConfig } from 'vite';

// Standalone IIFE build of the embed script. Output lands in `build/embed/`
// so the adapter-static output carries it verbatim into assets/dist.
export default defineConfig({
	build: {
		outDir: 'build/embed',
		emptyOutDir: false,
		target: 'es2019',
		lib: {
			entry: 'src/embed/main.js',
			name: 'statesuEmbed',
			formats: ['iife'],
			fileName: () => 'v1.js'
		}
	}
});
