<!--
  The session page (`#/s/<host>/<id>`).

  The static report's panels over the same facts document the report is
  rendered from, plus the two things the report cannot do: pan and zoom the
  timeline, and open what is behind a piece of it.

  **Two documents, fetched separately and on purpose.** The facts render the
  whole page; the events are the detail tier and are fetched alongside them,
  with their size on screen while they arrive — a twelve-hour session's events
  run to megabytes where its facts are ~100 KB. Everything above the timeline
  is drawn before they land, and the timeline itself is drawn at the facts'
  own resolution until they do. The page is never blocked on the big file.

  **A third, fetched only if asked for.** The calls document — what each tool
  call said — is bigger again (190 KB at the median against the events' 42 KB)
  and most trees do not carry it at all, because `derive` writes it only when
  asked. So nothing here touches it until a reader opens a call, and then it
  costs two fetches: `sessions.json`, whose `calls` link is the contract's own
  answer to "does this tree have any", and the document itself. A reader who
  never opens a call pays for neither.

  **The selection lives in the hash**, so a view can be pasted into korg:
  `#/s/<host>/<id>?phase=3`, or `?span=0&from=120&to=150` for a window. It is
  read on arrival and the timeline frames it.
-->
<script lang="ts">
	import { page } from '$app/state';
	import Panels from '$lib/components/Panels.svelte';
	import Segment from '$lib/components/Segment.svelte';
	import Timeline from '$lib/components/Timeline.svelte';
	import Written from '$lib/components/Written.svelte';
	import type { Facts } from '$lib/contract/facts.js';
	import type { EventsDocument } from '$lib/contract/events.js';
	import type { CallsDocument } from '$lib/contract/calls.js';
	import { toolFailureRate, dominantPhase, totalToolFailures } from '$lib/contract/derived.js';
	import { fromQuery, toQuery, type Selection } from '$lib/segment.js';
	import {
		loadFacts,
		loadEventsProgressively,
		loadCalls,
		loadSessions,
		reportUrl,
		type Progress
	} from '$lib/data.js';
	import { bytes, count, duration, percent, stamp } from '$lib/format.js';

	const host = $derived(page.params.host ?? '');
	const id = $derived(page.params.id ?? '');

	let facts = $state<Facts | undefined>(undefined);
	let error = $state<string | undefined>(undefined);
	let loading = $state(true);

	let events = $state<EventsDocument | undefined>(undefined);
	let progress = $state<Progress | undefined>(undefined);
	let eventsSize = $state<number | undefined>(undefined);
	let eventsError = $state<string | undefined>(undefined);

	let calls = $state<CallsDocument | undefined>(undefined);
	let callsState = $state<'unasked' | 'loading' | 'ready' | 'absent' | 'error'>('unasked');
	let callsError = $state<string | undefined>(undefined);

	let selection = $state<Selection | undefined>(undefined);
	let frame = $state<Selection | undefined>(undefined);

	/**
	 * Fetch the payload tier, once, because a reader asked to read a call.
	 *
	 * The index first, and not to find the path — that is
	 * `calls/<host>/<id>.json` and always has been. It is to find out whether
	 * there is one: `sessions.json` links `calls` only where `derive` was
	 * asked to write it, so its absence is the contract's own way of saying
	 * this tree carries no call text. Guessing the path and reading a 404 as
	 * "none" would conflate that with a tree whose derive half-finished.
	 */
	async function openCalls() {
		if (callsState !== 'unasked') return;
		const [h, i] = [host, id];
		callsState = 'loading';
		callsError = undefined;
		try {
			const index = await loadSessions();
			const entry = index.sessions.find((e) => e.host === h && e.session_id === i);
			if (!entry?.calls) {
				callsState = 'absent';
				return;
			}
			const doc = await loadCalls(h, i);
			// The session may have changed under a slow fetch.
			if (h !== host || i !== id) return;
			calls = doc;
			callsState = 'ready';
		} catch (e) {
			callsError = e instanceof Error ? e.message : String(e);
			callsState = 'error';
		}
	}

	$effect(() => {
		const [h, i] = [host, id];
		let live = true;
		loading = true;
		error = undefined;
		events = undefined;
		eventsError = undefined;
		eventsSize = undefined;
		calls = undefined;
		callsState = 'unasked';
		callsError = undefined;
		progress = { read: 0 };
		loadFacts(h, i)
			.then((f) => {
				if (live) facts = f;
			})
			.catch((e) => {
				if (live) error = e instanceof Error ? e.message : String(e);
			})
			.finally(() => {
				if (live) loading = false;
			});
		loadEventsProgressively(h, i, (p) => {
			if (live) {
				progress = p;
				eventsSize = p.read;
			}
		})
			.then((e) => {
				if (live) events = e;
			})
			.catch((e) => {
				if (live) eventsError = e instanceof Error ? e.message : String(e);
			})
			.finally(() => {
				if (live) progress = undefined;
			});
		return () => {
			live = false;
		};
	});

	/**
	 * The selection lives in the fragment, read once on arrival and written
	 * back on every change.
	 *
	 * Both halves go through `location.hash` and `history.replaceState` rather
	 * than kit's router, for the reason sprint 011 wrote down: `resolve()`
	 * returns `base + '#' + path` where `base` is the runtime-computed
	 * *directory*, so a resolved href points at the directory rather than the
	 * shell inside it — which copyparty serves as a file listing. Owning the
	 * fragment directly also means no navigation fires, so writing the
	 * selection back cannot re-run this page and discard it. Nothing else
	 * reads the query, so `page.url` going stale behind us costs nothing.
	 */
	let read = $state(false);
	$effect(() => {
		if (read || typeof location === 'undefined') return;
		read = true;
		const q = location.hash.indexOf('?');
		const want = fromQuery(new URLSearchParams(q < 0 ? '' : location.hash.slice(q + 1)));
		if (want) {
			selection = want;
			frame = want;
		}
	});

	$effect(() => {
		if (!read || typeof location === 'undefined') return;
		const q = toQuery(selection).toString();
		const cut = location.hash.indexOf('?');
		const route = cut < 0 ? location.hash : location.hash.slice(0, cut);
		const next = `${route}${q ? `?${q}` : ''}`;
		if (location.hash !== next) history.replaceState(history.state, '', next);
	});

	const title = $derived(
		facts
			? `${facts.project ?? 'session'}${facts.started ? ` — ${facts.started.slice(0, 10)}` : ''}`
			: id
	);

	// The same three-part note the report's `tools` stat carries: the count
	// says nothing without its denominator, and delegated calls are named here
	// rather than only in the tier below.
	const toolsNote = $derived.by(() => {
		if (!facts) return '';
		const failures = totalToolFailures(facts);
		const rate = toolFailureRate(facts);
		const failed =
			failures === 0
				? 'none failed'
				: rate !== undefined
					? `${failures} failed · ${percent(rate)}`
					: `${failures} failed`;
		const delegated = Object.values(facts.delegation.totals.tool_calls).reduce((n, v) => n + v, 0);
		return delegated > 0 ? `${failed} · ${delegated} more delegated` : failed;
	});
</script>

<svelte:head><title>kagviz — {title}</title></svelte:head>

<p class="crumbs"><a href="#/">← all sessions</a></p>

{#if error}
	<h1>{id}</h1>
	<p class="warn">Could not read this session's facts: {error}</p>
{:else if loading || !facts}
	<h1>{id}</h1>
	<p class="note">Reading the facts…</p>
{:else}
	<header>
		<h1>{title}</h1>
		<dl class="meta">
			<dt>host</dt>
			<dd>{host}</dd>
			<dt>session</dt>
			<dd>{facts.session_id ?? id}</dd>
			{#if facts.cwd}<dt>cwd</dt>
				<dd>{facts.cwd}</dd>{/if}
			{#if facts.git_branch}<dt>branch</dt>
				<dd>{facts.git_branch}</dd>{/if}
			{#if facts.cli_versions.length}<dt>cli</dt>
				<dd>{facts.cli_versions.join(', ')}</dd>{/if}
			<dt>models</dt>
			<dd>{Object.keys(facts.models).join(', ')}</dd>
			{#if facts.started && facts.ended}
				<dt>window</dt>
				<dd>{stamp(facts.started)} → {stamp(facts.ended)}</dd>
			{/if}
			<dt>report</dt>
			<dd>
				<a
					href={reportUrl(`reports/${host}/${facts.session_id ?? id}.html`)}
					target="_blank"
					rel="external noreferrer">static page</a
				>
			</dd>
		</dl>
	</header>

	{#if facts.skipped_lines > 0}
		<p class="warn">
			{facts.skipped_lines} transcript line(s) did not parse. Every number below is a partial reading.
		</p>
	{/if}

	<Written labels={facts.labels} />

	<section class="stats">
		<div class="stat">
			<span class="k">active</span>
			<span class="v">{duration(facts.active_secs)}</span>
			<span class="n">over {duration(facts.wall_secs)} wall · {duration(facts.idle_secs)} idle</span
			>
		</div>
		{#if dominantPhase(facts)}
			<div class="stat">
				<span class="k">phases</span>
				<span class="v">{facts.phases.length}</span>
				<span class="n">mostly {dominantPhase(facts)}</span>
			</div>
		{/if}
		<div class="stat">
			<span class="k">turns</span>
			<span class="v">{facts.assistant_turns}</span>
			<span class="n">{facts.user_prompts} user prompt(s)</span>
		</div>
		<div class="stat">
			<span class="k">tools</span>
			<span class="v">{count(Object.values(facts.tool_calls).reduce((n, v) => n + v, 0))}</span>
			<span class="n">{toolsNote}</span>
		</div>
	</section>

	{#if facts.activity.spans.length > 0}
		<section class="card">
			<h2>Time</h2>
			<p class="note">
				{duration(facts.active_secs)} of work in {facts.activity.spans.length} stretch(es). Idle gaps
				are collapsed to a break, so the axis is time worked, not time elapsed. Band colours name the
				phase kind — the Phases card below is the key, and every band names itself on hover.
			</p>
			<Timeline {facts} {events} bind:selection bind:frame />
			<p class="note detail">
				{#if eventsError}
					<span class="warn">Could not read this session's events: {eventsError}</span> The timeline still
					draws at the facts' own resolution; zooming past it, and opening a segment, need that document.
				{:else if events}
					Events read{eventsSize ? ` — ${bytes(eventsSize)}` : ''}. Click a column for the turns and
					tool calls behind it, or a phase band for the phase.
				{:else if progress}
					Reading the events — {bytes(progress.read)}{progress.total
						? ` of ${bytes(progress.total)}`
						: ''} so far. The page does not wait for them; zooming past
					{duration(facts.activity.bucket_secs)} per column and opening a segment do.
				{/if}
			</p>
		</section>
	{/if}

	{#if selection && events}
		<Segment
			{facts}
			{events}
			{selection}
			onclear={() => (selection = undefined)}
			{calls}
			{callsState}
			{callsError}
			onopencalls={openCalls}
		/>
	{/if}

	<Panels
		{facts}
		onopenspawn={events
			? (i) => {
					selection = { kind: 'spawn', spawn: i };
					document.querySelector('section.seg')?.scrollIntoView({ block: 'nearest' });
				}
			: undefined}
	/>

	<footer>
		Read from this session's facts document. Every figure is computed from the transcript;
		{#if facts.labels}
			the text marked <em>written</em> is not. Times are UTC.
		{:else}
			nothing here is inferred. Times are UTC.
		{/if}
	</footer>
{/if}

<style>
	.crumbs {
		margin: 0 0 12px;
		font-size: 13px;
	}
	header {
		border-bottom: 1px solid var(--line);
		padding-bottom: 20px;
		margin-bottom: 20px;
	}
	header dl.meta {
		max-width: 96ch;
	}
	footer {
		margin-top: 24px;
		font-size: 12px;
		color: var(--muted);
	}
</style>
