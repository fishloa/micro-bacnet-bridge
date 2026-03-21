import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [sveltekit()],
	build: {
		// Merge all JS into as few chunks as possible to avoid overwhelming
		// the W5500's limited concurrent TCP socket count (4 hardware sockets).
		rollupOptions: {
			output: {
				// Force all code into a single vendor chunk + entry chunk.
				manualChunks: () => 'app',
			},
		},
		cssCodeSplit: false,
	},
});
