/**
 * Durations, counts and percentages, exactly as `src/fmt.rs` renders them.
 *
 * Presentation only — nothing here computes a fact. The point of mirroring
 * `fmt.rs` rather than reaching for `Intl` is that the app and the static
 * report must not render one session's numbers two different ways: `3h05m` in
 * one place and `3 hr 5 min` in the other reads as two measurements. There is
 * a test beside this file holding it to `fmt.rs`'s own cases.
 */

/** `42s`, `7m`, `3h05m`, `54d01h` — the day rung is for resumed sessions. */
export function duration(secs: number): string {
	const s = Math.trunc(secs);
	if (s < 60) return `${s}s`;
	const mins = Math.trunc(s / 60);
	if (mins < 60) return `${mins}m`;
	const hours = Math.trunc(mins / 60);
	if (hours < 24) return `${hours}h${pad(mins % 60)}m`;
	return `${Math.trunc(hours / 24)}d${pad(hours % 24)}h`;
}

/** Thousands separators, so six-digit token totals stay readable. */
export function count(n: number): string {
	const digits = Math.trunc(n).toString();
	const neg = digits.startsWith('-');
	const body = neg ? digits.slice(1) : digits;
	let out = '';
	for (let i = 0; i < body.length; i++) {
		if (i > 0 && (body.length - i) % 3 === 0) out += ',';
		out += body[i];
	}
	return neg ? `-${out}` : out;
}

/**
 * A ratio as `1.62%`. Two decimals, fixed: at the low end — where most
 * sessions sit — `1.6%` folds a good session and a rough one into the same
 * figure, and a fixed width keeps the rate readable as a column.
 */
export function percent(ratio: number): string {
	return `${(ratio * 100).toFixed(2)}%`;
}

/** `+9/−2`, with the deltas coloured by the caller. */
export function signed(n: number): string {
	return n < 0 ? `−${count(-n)}` : `+${count(n)}`;
}

/** `2026-08-20 10:00 UTC` — the report's own stamp, and always UTC. */
export function stamp(iso: string | undefined): string {
	const d = parseAt(iso);
	if (!d) return '—';
	return `${d.toISOString().slice(0, 10)} ${d.toISOString().slice(11, 16)} UTC`;
}

/** `10:00` — a time within a session, for the strip's axis. */
export function clock(iso: string | undefined): string {
	const d = parseAt(iso);
	return d ? d.toISOString().slice(11, 16) : '—';
}

/** Milliseconds since the epoch, or `undefined` for an absent/unparseable stamp. */
export function epoch(iso: string | undefined): number | undefined {
	return parseAt(iso)?.getTime();
}

function parseAt(iso: string | undefined): Date | undefined {
	if (!iso) return undefined;
	const d = new Date(iso);
	return Number.isNaN(d.getTime()) ? undefined : d;
}

function pad(n: number): string {
	return n < 10 ? `0${n}` : `${n}`;
}
