import { defineConfig } from 'vitest/config';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// copyparty serves files, not SPA fallbacks: a GET for
			// `/kagviz/app/s/kai/<id>` is a 404, not `index.html`. So the route
			// lives in the fragment, which the server never sees, and one
			// `index.html` is the whole app. SvelteKit turns SSR off for `hash`
			// on its own and emits that single shell — which is why the adapter
			// takes no `fallback` here: a fallback page cannot know how deep it
			// is mounted and so is written with absolute asset URLs, while the
			// shell, being a real page at a known path, gets relative ones.
			router: { type: 'hash' },
			// The bundle is mounted under `/kagviz/app/` on copyparty and opened
			// from a file:// checkout in dev-build smoke tests. Relative asset
			// URLs make the mount point a property of where it was copied rather
			// than of the build.
			paths: { relative: true },
			adapter: adapter()
		})
	],
	test: {
		expect: { requireAssertions: true },
		projects: [
			{
				extends: './vite.config.ts',
				test: {
					name: 'unit',
					environment: 'node',
					include: ['src/**/*.{test,spec}.{js,ts}', 'scripts/**/*.{test,spec}.{js,ts}'],
					exclude: ['src/**/*.svelte.{test,spec}.{js,ts}']
				}
			}
		]
	}
});
