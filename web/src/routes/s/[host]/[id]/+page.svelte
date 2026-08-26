<!--
  The session page (`#/s/<host>/<id>`).

  The static report's panels, over the same facts document the report is
  rendered from — this page fetches `derived/facts/<host>/<id>.json` and reads
  nothing else. The renderer reads the facts, never the transcript, and so does
  this. The strip is drawn once and is static: pan, zoom and the click into a
  segment are part 2.
-->
<script lang="ts">
	import { page } from '$app/state';
	import Panels from '$lib/components/Panels.svelte';
	import Strip from '$lib/components/Strip.svelte';
	import Written from '$lib/components/Written.svelte';
	import type { Facts } from '$lib/contract/facts.js';
	import { toolFailureRate, dominantPhase, totalToolFailures } from '$lib/contract/derived.js';
	import { loadFacts, reportUrl } from '$lib/data.js';
	import { count, duration, percent, stamp } from '$lib/format.js';

	const host = $derived(page.params.host ?? '');
	const id = $derived(page.params.id ?? '');

	let facts = $state<Facts | undefined>(undefined);
	let error = $state<string | undefined>(undefined);
	let loading = $state(true);

	$effect(() => {
		const [h, i] = [host, id];
		let live = true;
		loading = true;
		error = undefined;
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
		return () => {
			live = false;
		};
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
				{duration(facts.active_secs)} of work in {facts.activity.spans.length} stretch(es),
				{duration(facts.activity.bucket_secs)} per column. Idle gaps are collapsed to a break.
			</p>
			<p class="note">
				Band colours name the phase kind — the Phases card below is the key, and every band names
				itself on hover.
			</p>
			<Strip {facts} />
		</section>
	{/if}

	<Panels {facts} />

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
