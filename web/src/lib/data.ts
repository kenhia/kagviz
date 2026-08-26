/**
 * Where the app reads its three documents from, and how.
 *
 * The bundle is deployed to `derived/app/` and served by copyparty at
 * `/kagviz/app/index.html`; the data sits one level up, at `/kagviz/`. So the
 * derived root is `../` **relative to the document**, not to the route — which
 * is the second reason the router is hash-based: the document URL never
 * changes as you navigate, so a relative fetch resolves the same way on the
 * browser page and on a session page. A path-routed SPA at
 * `/kagviz/app/s/kai/<id>` would have to know how deep it was.
 *
 * `KAGVIZ_DERIVED` overrides it for `vite dev` (there is no derived tree beside
 * the dev server) — see `web/README.md`.
 */

import { decodeSessions, type SessionsIndex } from './contract/sessions.js';
import { decodeSyncStatus, type SyncStatus } from './contract/sessions.js';
import { decodeFacts, type Facts } from './contract/facts.js';
import { decodeEvents, type EventsDocument } from './contract/events.js';
import { ContractError, parse } from './contract/decode.js';

/** Trailing slash always, so `new URL(path, root)` behaves. */
export const DERIVED_ROOT: string = import.meta.env.VITE_KAGVIZ_DERIVED || '../';

function url(path: string): string {
	return new URL(path, new URL(DERIVED_ROOT, document.baseURI)).toString();
}

/**
 * Fetch and decode, or throw something a person can act on.
 *
 * A 404 and a document this app cannot read are different failures and say so
 * differently: one means the derive has not reached this session, the other
 * means kagviz and the app have drifted. Neither renders as an empty panel.
 */
async function load<T>(path: string, decode: (raw: unknown, path: string) => T): Promise<T> {
	const target = url(path);
	let res: Response;
	try {
		res = await fetch(target);
	} catch (e) {
		throw new Error(`could not reach ${path}: ${e instanceof Error ? e.message : String(e)}`, {
			cause: e
		});
	}
	if (!res.ok) {
		throw new Error(`${path} — ${res.status} ${res.statusText}`);
	}
	const text = await res.text();
	try {
		return decode(parse(text, path), path);
	} catch (e) {
		if (e instanceof ContractError) {
			throw new Error(
				`${path} is not a document this app understands (${e.message}). ` +
					`The app and the kagviz that derived this tree have drifted — see docs/facts-contract.md.`,
				{ cause: e }
			);
		}
		throw e;
	}
}

export function loadSessions(): Promise<SessionsIndex> {
	return load('sessions.json', decodeSessions);
}

export function loadFacts(host: string, id: string): Promise<Facts> {
	return load(`facts/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`, decodeFacts);
}

export function loadEvents(host: string, id: string): Promise<EventsDocument> {
	return load(`events/${encodeURIComponent(host)}/${encodeURIComponent(id)}.json`, decodeEvents);
}

/**
 * The sync status, or `undefined` when the tree carries none.
 *
 * Absent is not "everything was reached": the page says "no sync status" in
 * the place the host list would be, for the same reason the collector writes
 * the file at all.
 */
export async function loadSyncStatus(): Promise<SyncStatus | undefined> {
	try {
		return await load('sync-status.json', decodeSyncStatus);
	} catch {
		return undefined;
	}
}

/** The static report for a row, on the same served tree. */
export function reportUrl(path: string): string {
	return url(path);
}
