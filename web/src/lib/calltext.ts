/**
 * What to show when a reader opens a tool call.
 *
 * Pure, and separate from `Segment.svelte`, for the reason the rest of this
 * tree is: the decisions here are the ones the contract cares about, and a
 * decision buried in markup is a decision nothing tests.
 *
 * All of them are the same decision wearing different clothes — **an unknown
 * is never rendered as a zero.** Three distinct things would otherwise all
 * come out as an empty box, and over the pinned corpus none of them is
 * theoretical:
 *
 * - a call that was **interrupted** and has no result at all (58 calls);
 * - a result that came back carrying **no text** (a real, known, empty result);
 * - a result whose content was **not text** — an image, a `tool_reference` —
 *   which `result_bytes` counts as zero because it counts text (4,672 calls).
 *
 * And a fourth that would otherwise read as a complete result when it is a
 * 2 KB excerpt: an **offloaded** one, where the harness saved the real output
 * beside the transcript and handed the model a preview (19 calls).
 */

import { utf8Length } from './contract/decode.js';
import type { Call } from './contract/calls.js';

/** Where the page's one lazy fetch has got to. */
export type CallsState = 'unasked' | 'loading' | 'ready' | 'absent' | 'error';

export type ResultView =
	/** No result arrived. Not an empty result — nothing came back at all. */
	| { kind: 'interrupted' }
	/** A result arrived carrying no text and nothing else either: a known zero. */
	| { kind: 'empty' }
	| { kind: 'text'; text: string };

export type CallView =
	/** This tree was derived without `--calls`. The default, and not a failure. */
	| { kind: 'absent' }
	| { kind: 'loading' }
	| { kind: 'error'; message: string }
	/** The documents disagree: a tool event with no entry. A defect, said so. */
	| { kind: 'unjoined'; id: string }
	| {
			kind: 'ready';
			/** Absent when the call carried no input — not an empty one. */
			input?: string;
			result: ResultView;
			/** Non-text block types in the result. Empty when there were none. */
			blocks: string[];
			/** Present when `result` is a preview; `bytes` absent if unrecorded. */
			persisted?: { bytes?: number };
	  };

/**
 * Resolve one row's expanded state.
 *
 * `state` is authoritative over `call`: a document still arriving must read as
 * loading even though there is nothing to show yet, and a tree with no call
 * text must say so rather than looking like a join that failed.
 */
export function callView(
	state: CallsState,
	call: Call | undefined,
	toolUseId: string,
	error?: string
): CallView {
	if (state === 'absent') return { kind: 'absent' };
	if (state === 'error') return { kind: 'error', message: error ?? 'unknown error' };
	if (state !== 'ready') return { kind: 'loading' };
	if (!call) return { kind: 'unjoined', id: toolUseId };

	return {
		kind: 'ready',
		input: call.input === undefined ? undefined : asText(call.input),
		result: resultView(call),
		blocks: call.result_blocks,
		persisted: call.persisted ? { bytes: call.persisted_bytes } : undefined
	};
}

function resultView(call: Call): ResultView {
	// Absent, not empty. This is the line the whole module exists for.
	if (call.result === undefined) return { kind: 'interrupted' };
	// An empty string with non-text blocks beside it is not an empty result —
	// it is a result this document does not carry, and `blocks` says which.
	if (call.result === '' && call.result_blocks.length === 0) return { kind: 'empty' };
	if (call.result === '') return { kind: 'text', text: '' };
	return { kind: 'text', text: call.result };
}

/**
 * An input as text, and *only* as text.
 *
 * A string input is its own text; anything else is formatted JSON. Not
 * rendered as markdown, HTML or highlighted source — a tool's input is
 * arbitrary and deciding what it "is" is a decision that goes wrong on a
 * shared screen.
 */
export function asText(input: unknown): string {
	return typeof input === 'string' ? input : JSON.stringify(input, null, 2);
}

/**
 * How much of a payload goes on screen before it has to be asked for again.
 *
 * Not a safety limit — the harness already bounds payloads, so the corpus
 * maximum is 85 KB and p99 is 11.8 KB. It is a reading one: one click should
 * not bury the next row under 85 KB of text.
 */
export const HEAD = 2000;

/**
 * The head of a payload, whether anything was held back, and **the size in
 * the units kagviz reports them in**.
 *
 * `bytes` is UTF-8, not `String.length`: the panel prints it beside figures
 * the events document produced, and two numbers side by side that count
 * differently is exactly the disagreement this app renders as a warning
 * everywhere else. Clipping itself is by code unit — it is a display bound,
 * and slicing UTF-8 by hand would only risk splitting a character.
 */
export function clip(
	text: string,
	full: boolean
): { shown: string; hidden: boolean; bytes: number } {
	const bytes = utf8Length(text);
	if (full || text.length <= HEAD) return { shown: text, hidden: false, bytes };
	return { shown: text.slice(0, HEAD), hidden: true, bytes };
}
