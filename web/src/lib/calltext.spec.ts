/**
 * The four readings an expanded call must keep apart, and one clip rule.
 *
 * Every case here is drawn from the 015 corpus sweep rather than invented:
 * 58 interrupted calls, 19 offloaded results and 4,672 non-text result blocks
 * across 45,394 calls. If any two of them collapse into "an empty box", the
 * panel has turned an unknown into a zero.
 */

import { describe, expect, it } from 'vitest';
import { callView, asText, clip, HEAD } from './calltext.js';
import type { Call } from './contract/calls.js';

function call(over: Partial<Call> = {}): Call {
	return { id: 't1', tool: 'Bash', result_blocks: [], persisted: false, ...over };
}

describe('the fetch state is authoritative over the document', () => {
	it('says a tree carries no call text rather than looking like a failure', () => {
		expect(callView('absent', undefined, 't1')).toEqual({ kind: 'absent' });
	});

	it('reads unasked and loading alike — there is nothing to show either way', () => {
		expect(callView('unasked', undefined, 't1').kind).toBe('loading');
		expect(callView('loading', undefined, 't1').kind).toBe('loading');
	});

	it('carries the error through instead of an empty panel', () => {
		expect(callView('error', undefined, 't1', 'boom')).toEqual({
			kind: 'error',
			message: 'boom'
		});
	});

	it('calls a failed join a defect, not an empty result', () => {
		expect(callView('ready', undefined, 'toolu_9')).toEqual({
			kind: 'unjoined',
			id: 'toolu_9'
		});
	});
});

describe('the readings an empty box would collapse', () => {
	it('keeps an interrupted call apart from an empty result', () => {
		const interrupted = callView('ready', call({ result: undefined }), 't1');
		const empty = callView('ready', call({ result: '' }), 't1');
		expect(interrupted).toMatchObject({ result: { kind: 'interrupted' } });
		expect(empty).toMatchObject({ result: { kind: 'empty' } });
		expect(interrupted).not.toEqual(empty);
	});

	/**
	 * The 4,672-call case. `result_bytes` counts text, so an image result is a
	 * legitimate zero — and rendering that zero alone would say "nothing came
	 * back" about a screenshot.
	 */
	it('does not call a non-text result an empty one', () => {
		const v = callView('ready', call({ result: '', result_blocks: ['image'] }), 't1');
		expect(v).toMatchObject({ kind: 'ready', blocks: ['image'] });
		expect(v).not.toMatchObject({ result: { kind: 'empty' } });
	});

	it('marks an offloaded result as a preview, with the size it stands for', () => {
		const v = callView(
			'ready',
			call({ result: '<persisted-output>…', persisted: true, persisted_bytes: 227547 }),
			't1'
		);
		expect(v).toMatchObject({ persisted: { bytes: 227547 } });
	});

	/** A path with no size is an unknown; it must not become a zero here. */
	it('keeps an unrecorded offload size absent rather than zero', () => {
		const v = callView('ready', call({ result: 'x', persisted: true }), 't1');
		expect(v).toMatchObject({ persisted: {} });
		expect((v as { persisted: { bytes?: number } }).persisted.bytes).toBeUndefined();
	});

	it('leaves a call with no input absent rather than showing an empty one', () => {
		expect(callView('ready', call({ input: undefined, result: 'x' }), 't1')).toMatchObject({
			input: undefined
		});
		expect(callView('ready', call({ input: '', result: 'x' }), 't1')).toMatchObject({
			input: ''
		});
	});
});

describe('an input is shown as text and nothing else', () => {
	it('leaves a string input alone and formats anything else as JSON', () => {
		expect(asText('git status')).toBe('git status');
		expect(asText({ command: 'ls' })).toBe('{\n  "command": "ls"\n}');
	});

	/** `null` is a real input if the transcript carried one, not an absence. */
	it('renders a null input rather than dropping it', () => {
		expect(asText(null)).toBe('null');
	});
});

describe('clipping', () => {
	it('holds nothing back below the head', () => {
		expect(clip('abc', false)).toEqual({ shown: 'abc', hidden: false, bytes: 3 });
		const exact = 'x'.repeat(HEAD);
		expect(clip(exact, false)).toMatchObject({ shown: exact, hidden: false });
	});

	it('holds back the tail above it, and says so', () => {
		const long = 'x'.repeat(HEAD + 1);
		expect(clip(long, false)).toMatchObject({ shown: 'x'.repeat(HEAD), hidden: true });
		expect(clip(long, true)).toMatchObject({ shown: long, hidden: false });
	});

	/**
	 * The size it reports is the size the events document reported, in the
	 * same units. `String.length` would be UTF-16 code units and would
	 * disagree with `result_bytes` on over half the corpus's tool results —
	 * a second number for the same text, on the same screen.
	 */
	it('measures in UTF-8 bytes, not UTF-16 code units', () => {
		const text = '3:rsync -a src/ dest/  \u2014 1 match, 100\u00b5s';
		expect(text.length).toBe(39);
		expect(clip(text, false).bytes).toBe(42);
	});

	/** And the whole payload's size, not the clipped head's. */
	it('reports the full size even when it held the tail back', () => {
		const long = '\u00e9'.repeat(HEAD + 1);
		expect(clip(long, false)).toMatchObject({ hidden: true, bytes: (HEAD + 1) * 2 });
	});
});
