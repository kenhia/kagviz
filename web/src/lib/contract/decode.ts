/**
 * The small decoder kit the three contracts are built from.
 *
 * Not `as` casts. `docs/facts-contract.md` says an unknown is never rendered
 * as a zero; a cast turns a document this app does not understand into zeros
 * and empty panels, which is the same lie one layer down. So every required
 * field is checked, and a document that fails the check throws with the path
 * that failed.
 *
 * Two rules from the contract are encoded here rather than restated at each
 * call site:
 *
 * - **Absent and `null` are the same thing to a consumer.** kagviz has emitted
 *   absent-only since sprint 009, and the conformance test holds it to that —
 *   but the contract tells a *consumer* to treat the two alike, so `optional`
 *   folds `null` into `undefined` rather than throwing on it.
 * - **Unknown fields are ignored.** Adding a field is not a breaking change,
 *   so decoding reads the fields it knows and passes nothing else through.
 */

export class ContractError extends Error {
	constructor(
		readonly path: string,
		readonly detail: string
	) {
		super(`${path}: ${detail}`);
		this.name = 'ContractError';
	}
}

function fail(path: string, detail: string): never {
	throw new ContractError(path, detail);
}

/** A JSON object, or a throw naming what arrived instead. */
export function object(v: unknown, path: string): Record<string, unknown> {
	if (typeof v !== 'object' || v === null || Array.isArray(v)) {
		fail(path, `expected an object, got ${describe(v)}`);
	}
	return v as Record<string, unknown>;
}

export function str(o: Record<string, unknown>, key: string, path: string): string {
	const v = o[key];
	if (typeof v !== 'string') fail(`${path}.${key}`, `expected a string, got ${describe(v)}`);
	return v;
}

/** A number that must be present. Non-finite is a broken document, not a zero. */
export function num(o: Record<string, unknown>, key: string, path: string): number {
	const v = o[key];
	if (typeof v !== 'number' || !Number.isFinite(v)) {
		fail(`${path}.${key}`, `expected a number, got ${describe(v)}`);
	}
	return v;
}

export function bool(o: Record<string, unknown>, key: string, path: string): boolean {
	const v = o[key];
	if (typeof v !== 'boolean') fail(`${path}.${key}`, `expected a boolean, got ${describe(v)}`);
	return v;
}

/**
 * A count that the producer may legitimately omit — `#[serde(default)]` on the
 * Rust side, `failed`/`opaque`/`tools` in the events document. Absent means
 * the default, which for these is genuinely zero and not an unknown.
 */
export function numOr(o: Record<string, unknown>, key: string, path: string, dflt: number): number {
	return has(o, key) ? num(o, key, path) : dflt;
}

export function boolOr(
	o: Record<string, unknown>,
	key: string,
	path: string,
	dflt: boolean
): boolean {
	return has(o, key) ? bool(o, key, path) : dflt;
}

export function optStr(o: Record<string, unknown>, key: string, path: string): string | undefined {
	return has(o, key) ? str(o, key, path) : undefined;
}

export function optNum(o: Record<string, unknown>, key: string, path: string): number | undefined {
	return has(o, key) ? num(o, key, path) : undefined;
}

export function arr<T>(
	o: Record<string, unknown>,
	key: string,
	path: string,
	each: (v: unknown, path: string) => T
): T[] {
	const v = o[key];
	if (!Array.isArray(v)) fail(`${path}.${key}`, `expected an array, got ${describe(v)}`);
	return v.map((item, i) => each(item, `${path}.${key}[${i}]`));
}

/** An array the producer may omit entirely — absent reads as empty. */
export function arrOr<T>(
	o: Record<string, unknown>,
	key: string,
	path: string,
	each: (v: unknown, path: string) => T
): T[] {
	return has(o, key) ? arr(o, key, path, each) : [];
}

export function strItem(v: unknown, path: string): string {
	if (typeof v !== 'string') fail(path, `expected a string, got ${describe(v)}`);
	return v;
}

/** `tool_calls`, `tool_failures`, `models` — a name → count map. */
export function counts(o: Record<string, unknown>, key: string, path: string): Counts {
	const v = object(o[key] ?? {}, `${path}.${key}`);
	const out: Counts = {};
	for (const [name, n] of Object.entries(v)) {
		if (typeof n !== 'number' || !Number.isFinite(n)) {
			fail(`${path}.${key}.${name}`, `expected a number, got ${describe(n)}`);
		}
		out[name] = n;
	}
	return out;
}

export type Counts = Record<string, number>;

/** Sum a `Counts` map — every "how many calls in total" on the page. */
export function sum(c: Counts): number {
	let total = 0;
	for (const n of Object.values(c)) total += n;
	return total;
}

/**
 * Present and not `null`. The contract's absent-never-null rule is a promise
 * the *producer* keeps; a consumer treats the two the same, so this is the
 * one place that decision lives.
 */
function has(o: Record<string, unknown>, key: string): boolean {
	return o[key] !== undefined && o[key] !== null;
}

function describe(v: unknown): string {
	if (v === null) return 'null';
	if (v === undefined) return 'nothing';
	if (Array.isArray(v)) return 'an array';
	return typeof v;
}

/** Parse JSON text into a document, naming the document in any failure. */
export function parse(text: string, what: string): unknown {
	try {
		return JSON.parse(text);
	} catch (e) {
		throw new ContractError(what, `not JSON: ${e instanceof Error ? e.message : String(e)}`);
	}
}
