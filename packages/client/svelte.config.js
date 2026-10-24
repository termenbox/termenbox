import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

// Lets svelte-check (and editors) read the TypeScript in .svelte files; the build gets the same from vite.config.ts.
export default { preprocess: vitePreprocess() };
