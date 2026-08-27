/**
 * The calls document — `kagviz show <id> --calls`, and
 * `derived/calls/<host>/<id>.json`.
 *
 * The payload tier under the events: what each tool call actually said, and
 * what came back. A fourth contract under the same rules, added in sprint 015.
 *
 * **It is the only one that may not be there**, and that is the contract, not
 * a gap. Everything else kagviz derives is counted *from* the transcript;
 * this is the transcript's own text — command output, file contents, pasted
 * material, potentially credentials — so `derive` writes it only when asked.
 *
 * `sessions.json` carries `calls` only when the tree has one, and **that is
 * the signal, not a 404**. The path is deterministic and always was; what the
 * index answers is whether there is anything at it. Reading a missing file as
 * "this tree carries no call text" would conflate a deliberate default with a
 * derive that half-finished, which is the same unknown-as-a-zero this
 * contract refuses everywhere else.
 *
 * Never fetch it speculatively, and never alongside the events.
 *
 * The invariants a consumer can lean on (`conformance.spec.ts` asserts them
 * over the repo's goldens):
 *
 * - one entry per `tool` event across the events document's own tier **and**
 *   all of its spawns;
 * - `utf8Length(JSON.stringify(input))` is the event's `input_bytes`, and
 *   `utf8Length(result)` is its `result_bytes` — both documents are filled
 *   from the same block in the same pass, so these hold by construction.
 *   **`utf8Length`, not `.length`**: every size kagviz reports is UTF-8 bytes
 *   and `String.length` counts UTF-16 code units. They agree only on ASCII,
 *   and over half the corpus's tool results are not ASCII;
 * - `input`/`result` are present exactly when `input_bytes`/`result_bytes`
 *   are.
 *
 * The one reading this document exists to protect: **absent is not empty.**
 * A call with no `result` was interrupted or was still running; a call whose
 * `result` is `''` came back carrying no text. A panel that draws both as an
 * empty box has turned an unknown into a zero, which is the thing the whole
 * contract is built to refuse.
 */

import { arrOr, object, optNum, optStr, str, strItem } from './decode.js';

export interface Call {
	/**
	 * The join key back to the events' `tool` events. **Absent** when the
	 * transcript carried none — the entry is still here so the document does
	 * not under-report, but nothing can look it up.
	 */
	id?: string;
	/** Duplicated from the event, so an entry with no `id` is not nameless. */
	tool: string;
	/** The input as the model was handed it. Absent when the block had none. */
	input?: unknown;
	/**
	 * The result's text as the model was handed it. **Absent** when no result
	 * arrived; present and `''` when one arrived carrying no text. Different
	 * readings — see the note above.
	 */
	result?: string;
	/**
	 * Block types in the result that carried no text — `tool_reference`,
	 * `image`. Empty when there were none. This is what stops an empty
	 * `result` from having to read as an empty result.
	 */
	result_blocks: string[];
	/**
	 * The harness offloaded the real output and handed the model a ~2 KB
	 * preview: `result` **is that preview, not the output**.
	 */
	persisted: boolean;
	/**
	 * What the harness recorded as the real output's size. **Absent** when it
	 * recorded a path but no size — an unknown, not a zero. The offloaded file
	 * itself is not served.
	 */
	persisted_bytes?: number;
}

export interface CallsDocument {
	session_id?: string;
	/** Flat: the session's own tier, then every spawn's. Joined by `id`. */
	calls: Call[];
}

export function decodeCalls(raw: unknown, path = 'calls'): CallsDocument {
	const o = object(raw, path);
	return {
		session_id: optStr(o, 'session_id', path),
		calls: arrOr(o, 'calls', path, decodeCall)
	};
}

export function decodeCall(raw: unknown, path: string): Call {
	const o = object(raw, path);
	return {
		id: optStr(o, 'id', path),
		tool: str(o, 'tool', path),
		// `input` is arbitrary JSON — whatever the tool takes — so there is
		// nothing to check beyond presence. `has` rather than `!= null`,
		// because an input that really was `null` is still an input.
		input: 'input' in o ? o['input'] : undefined,
		result: optStr(o, 'result', path),
		result_blocks: arrOr(o, 'result_blocks', path, strItem),
		persisted: o['persisted'] === true,
		persisted_bytes: optNum(o, 'persisted_bytes', path)
	};
}

/** The calls of one document, by `tool_use_id`, for joining to the events. */
export function byId(doc: CallsDocument): Map<string, Call> {
	const m = new Map<string, Call>();
	for (const c of doc.calls) if (c.id !== undefined) m.set(c.id, c);
	return m;
}
