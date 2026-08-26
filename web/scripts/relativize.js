/**
 * Make the built shell independent of where it is mounted.
 *
 * SvelteKit's `paths.relative` rewrites asset URLs on a page it prerenders,
 * because it knows how deep that page sits. The hash-routing shell is
 * generated as a *fallback* — a page with no path of its own — so kit
 * deliberately leaves its `<link>` and `import()` URLs absolute (`/_app/…`)
 * and only makes the runtime base relative. Mounted at `/kagviz/app/` on
 * copyparty, those absolute URLs resolve against the server root and the app
 * never boots.
 *
 * Hash routing is exactly the condition that makes the fix safe: the document
 * is always `index.html` at the deployment root and its URL never changes as
 * you navigate, so `./_app/…` is correct wherever the directory is copied. The
 * app then works at `/kagviz/app/`, under `npm run preview`, and from a plain
 * `file://` open, with nothing baked in about the mount point.
 *
 * This runs as `postbuild`, so `just check`'s build gate covers it, and it
 * **throws** rather than warning: a kit release that changes this output must
 * fail the build, not ship a page that silently cannot start.
 */

const ABSOLUTE = /(["'(])\/_app\//g;

/**
 * @param {string} html
 * @returns {{ html: string, replaced: number }}
 */
export function relativize(html) {
	let replaced = 0;
	const out = html.replace(ABSOLUTE, (_m, quote) => {
		replaced += 1;
		return `${quote}./_app/`;
	});
	return { html: out, replaced };
}

/**
 * @param {string} html
 * @returns {string[]} the absolute `/_app/` references still present
 */
export function absoluteRefs(html) {
	return [...html.matchAll(/["'(](\/_app\/[^"')]*)/g)].map((m) => m[1]);
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/'))) {
	const { readFileSync, writeFileSync } = await import('node:fs');
	const target = process.argv[2] ?? 'build/index.html';
	const source = readFileSync(target, 'utf8');
	const { html, replaced } = relativize(source);
	if (replaced === 0) {
		throw new Error(
			`${target} has no absolute /_app/ references to rewrite. Either SvelteKit now emits ` +
				`relative URLs for the hash-routing shell — in which case delete this step — or the ` +
				`shell is not where this expects it.`
		);
	}
	const left = absoluteRefs(html);
	if (left.length > 0) {
		throw new Error(
			`${target} still references ${left.length} absolute path(s): ${left.join(', ')}`
		);
	}
	writeFileSync(target, html);
	console.log(`relativized ${replaced} asset reference(s) in ${target}`);
}
