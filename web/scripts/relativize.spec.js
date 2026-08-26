import { describe, expect, it } from 'vitest';
import { absoluteRefs, relativize } from './relativize.js';

/**
 * The shapes SvelteKit actually emits into the hash-routing shell: a
 * `<link rel="modulepreload">`, a stylesheet link, and the two dynamic
 * `import()` calls that boot the app. All four have to move, and the last two
 * are the ones that matter — a page that loads its CSS and never starts is
 * worse than one that plainly 404s.
 */
const SHELL = `<link href="/_app/immutable/entry/start.abc.js" rel="modulepreload">
<link href="/_app/immutable/assets/0.def.css" rel="stylesheet">
<script>
	Promise.all([
		import("/_app/immutable/entry/start.abc.js"),
		import("/_app/immutable/entry/app.ghi.js")
	]).then(([kit, app]) => kit.start(app, element));
</script>`;

describe('relativize', () => {
	it('moves every absolute asset reference under the document', () => {
		const { html, replaced } = relativize(SHELL);
		expect(replaced).toBe(4);
		expect(absoluteRefs(html)).toEqual([]);
		expect(html).toContain('href="./_app/immutable/entry/start.abc.js"');
		expect(html).toContain('import("./_app/immutable/entry/app.ghi.js")');
	});

	it('leaves a path that is already relative alone', () => {
		const { replaced } = relativize(relativize(SHELL).html);
		expect(replaced).toBe(0);
	});

	it('does not touch a URL that merely contains the segment', () => {
		const html = '<a href="https://example.test/_app/docs">x</a>';
		expect(relativize(html).replaced).toBe(0);
		expect(absoluteRefs(html)).toEqual([]);
	});
});
