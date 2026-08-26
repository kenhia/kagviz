<!--
  The static report's panels, in the app.

  Every rule the report follows holds here, and each is a line of code rather
  than an intention:

  - written text is marked (`Written`, and the `written` chip beside a phase's
    mechanical kind — beside it, never in place of it);
  - unknowns are visibly absent (`opaque_edits` is called out under the file
    counts and never folded into them; an unjoined spawn's work is named as
    missing; a failure with no call to join says so);
  - nothing is computed that the contract does not allow a consumer to compute
    (the combined line and the failure rate come from `contract/derived.ts`,
    which makes the report's own two choices).
-->
<script lang="ts">
	import Bars from './Bars.svelte';
	import type { Facts } from '$lib/contract/facts.js';
	import { TOOL_CLASSES } from '$lib/contract/facts.js';
	import {
		combinedOutputTokens,
		combinedToolCalls,
		phaseLabel,
		phaseRollup,
		totalToolCalls,
		unknownFailures
	} from '$lib/contract/derived.js';
	import { clock, count, duration } from '$lib/format.js';

	let {
		facts,
		onopenspawn
	}: {
		facts: Facts;
		/**
		 * Open a spawn's own events. Absent until the events document is read —
		 * the row must not offer a door that opens onto nothing.
		 */
		onopenspawn?: (i: number) => void;
	} = $props();

	const PHASE_LIST_MAX = 30;
	const PHASE_LIST_LONGEST = 15;

	const rollup = $derived(phaseRollup(facts));
	const rollupRows = $derived.by(() => {
		const peak = Math.max(1, rollup[0]?.secs ?? 1);
		return rollup.map((r) => ({
			name: r.kind,
			pct: (r.secs * 100) / peak,
			kind: r.kind,
			note: `${duration(r.secs)} · ${r.phases} phase(s)`
		}));
	});

	// Carry the index: a written label is keyed by position in the facts'
	// `phases`, and this list sorts and truncates.
	const phaseList = $derived.by(() => {
		const all = facts.phases.map((p, i) => ({ i, p }));
		if (all.length <= PHASE_LIST_MAX) return { shown: all, omitted: 0 };
		const longest = [...all]
			.sort((a, b) => b.p.secs - a.p.secs || a.p.started.localeCompare(b.p.started))
			.slice(0, PHASE_LIST_LONGEST)
			.sort((a, b) => a.p.started.localeCompare(b.p.started));
		return { shown: longest, omitted: all.length - PHASE_LIST_LONGEST };
	});

	const toolRows = $derived.by(() => {
		const entries = Object.entries(facts.tool_calls);
		const peak = Math.max(1, ...entries.map(([, n]) => n));
		return entries
			.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
			.map(([name, n]) => {
				const failed = facts.tool_failures[name] ?? 0;
				return {
					name,
					pct: (Math.max(0, n - failed) * 100) / peak,
					failPct: (failed * 100) / peak,
					note: `${n}`,
					emphasis: failed > 0 ? `${failed} failed` : undefined
				};
			});
	});

	const byTool = $derived(
		Object.entries(facts.changes.by_tool).sort(
			(a, b) => b[1].calls - a[1].calls || a[0].localeCompare(b[0])
		)
	);

	function mixLine(mix: Record<string, number>): string {
		return TOOL_CLASSES.map((c) => [c, mix[c] ?? 0] as const)
			.filter(([, n]) => n > 0)
			.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
			.map(([c, n]) => `${c} ${n}`)
			.join(' · ');
	}
</script>

{#if facts.phases.length > 0}
	<section class="card">
		<h2>Phases</h2>
		<p class="note">
			{facts.phases.length} phase(s), cut at every user turn and at every idle break. A phase never spans
			a gap, and the phases in one stretch of work account for all of it. Labels name the tool mix, not
			the intent.
		</p>
		<Bars rows={rollupRows} />
		{#if phaseList.omitted > 0}
			<p class="note">
				Listing the {PHASE_LIST_LONGEST} longest; {phaseList.omitted} shorter phase(s) are not shown.
				The band on the strip above is the full sequence.
			</p>
		{/if}
		<ol class="turns">
			{#each phaseList.shown as { i, p } (i)}
				<li>
					<span class="when">{clock(p.started)}</span>
					<span class="what">
						<span class="kindchip k-{p.kind}">{p.kind}</span>
						<strong>{duration(p.secs)}</strong>
						{#if p.opened_by}
							— {p.opened_by}
						{:else}
							— <em class="resumed">resumed, nothing said</em>
						{/if}
						{#if phaseLabel(facts, i)}
							<span class="chip said">{phaseLabel(facts, i)}</span>
						{/if}
						{#if mixLine(p.mix)}
							<br /><span class="opts">{mixLine(p.mix)}</span>
						{/if}
					</span>
				</li>
			{/each}
		</ol>
	</section>
{/if}

<div class="cols">
	<section class="card">
		<h2>Tools</h2>
		{#if toolRows.length === 0}
			<p class="note">No tool calls in this session.</p>
		{:else}
			<Bars rows={toolRows} />
			{#if unknownFailures(facts) > 0}
				<p class="note">
					{unknownFailures(facts)} failure(s) could not be joined to a call — the matching
					<code>tool_use</code> is not in this transcript.
				</p>
			{/if}
		{/if}
	</section>

	<section class="card">
		<h2>Files</h2>
		<p class="big">
			{facts.changes.files_touched} file(s)
			<span class="add">+{count(facts.changes.lines_added)}</span>
			<span class="del">−{count(facts.changes.lines_deleted)}</span>
		</p>
		{#if byTool.length > 0}
			<ul class="sources">
				{#each byTool as [tool, t] (tool)}
					<li>
						<span class="name">{tool}</span>
						<span class="n">{t.calls}×</span>
						<span class="d">
							{#if t.opaque === t.calls}
								<span class="unseen">no readable diff</span>
							{:else}
								{t.files_touched} file(s)
								<span class="add">+{count(t.lines_added)}</span>
								<span class="del">−{count(t.lines_deleted)}</span>
								{#if t.opaque > 0}
									· <span class="unseen">{t.opaque} unreadable</span>
								{/if}
							{/if}
						</span>
					</li>
				{/each}
			</ul>
		{/if}
		{#if facts.changes.opaque_edits > 0}
			<p class="note unknown">
				{facts.changes.opaque_edits} call(s) could have changed files and left no recoverable diff. Those
				edits are <strong>not</strong> in the counts above — an unknown, not a zero.
			</p>
		{/if}
	</section>

	<section class="card">
		<h2>Tokens</h2>
		<dl class="meta">
			<dt>output</dt>
			<dd>{count(facts.tokens.output)}</dd>
			<dt>thinking</dt>
			<dd>{count(facts.tokens.thinking)}</dd>
			<dt>input</dt>
			<dd>{count(facts.tokens.input)}</dd>
			<dt>cache read</dt>
			<dd>{count(facts.tokens.cache_read)}</dd>
			<dt>cache write</dt>
			<dd>{count(facts.tokens.cache_write)}</dd>
		</dl>
	</section>
</div>

<section class="card">
	<h2>User involvement</h2>
	{#if facts.user_involvement.length === 0}
		<p class="note">
			The user did not speak after the opening prompt — this session ran unattended.
		</p>
	{:else}
		<p class="note">
			{facts.user_prompts} prompt(s), {facts.ask_user_questions} question(s) asked,
			{facts.pasted_attachments} pasted image(s)/document(s).
		</p>
		<ol class="turns">
			{#each facts.user_involvement as item, i (i)}
				<li class={item.kind}>
					<span class="when">{item.at ? clock(item.at) : ''}</span>
					<span class="what">
						{#if item.kind === 'prompt'}
							{item.preview || 'user pasted an attachment'}{item.truncated ? '…' : ''}
							{#if item.attachments > 0}
								<span class="chip">+{item.attachments} attached</span>
							{/if}
						{:else}
							{#if item.header}<span class="chip">{item.header}</span>{/if}
							<strong>{item.question}</strong>
							<br />
							<span class="opts">
								{#each item.options as opt, oi (oi)}
									<span class="opt" class:chosen={opt === item.chosen}>{opt}</span>
								{/each}
								{#if item.chosen === undefined}
									<em class="unanswered">no answer recorded</em>
								{/if}
							</span>
						{/if}
					</span>
				</li>
			{/each}
		</ol>
	{/if}
</section>

{#if facts.skills.length > 0 || facts.subagents.length > 0}
	<section class="card">
		<h2>Skills &amp; delegation</h2>
		<p>
			{#each facts.skills as skill (skill)}
				<span class="chip">/{skill}</span>
			{/each}
			{#each facts.subagents as agent (agent)}
				<span class="chip agent">{agent}</span>
			{/each}
		</p>
	</section>
{/if}

{#if facts.delegation.spawns.length > 0 || facts.delegation.unjoined_spawns > 0}
	<section class="card">
		<h2>Delegated work</h2>
		{#if facts.delegation.spawns.length > 0}
			<table class="tier">
				<thead>
					<tr>
						<th>agent</th>
						<th>what for</th>
						<th class="r">tools</th>
						<th class="r">active</th>
						<th class="r">output</th>
					</tr>
				</thead>
				<tbody>
					{#each facts.delegation.spawns as spawn, i (i)}
						{@const calls = Object.values(spawn.tool_calls).reduce((n, v) => n + v, 0)}
						{@const failed = Object.values(spawn.tool_failures).reduce((n, v) => n + v, 0)}
						<tr>
							<td>
								<span class="chip agent">{spawn.subagent_type ?? 'agent'}</span>
								{#if spawn.model}<span class="sub">{spawn.model}</span>{/if}
							</td>
							<td>
								{spawn.description ?? '—'}
								{#if onopenspawn}
									<button class="open" onclick={() => onopenspawn(i)}>events</button>
								{/if}
							</td>
							<td class="r"
								>{calls}{#if failed > 0}<em>{failed} failed</em>{/if}</td
							>
							<td class="r">{duration(spawn.active_secs)}</td>
							<td class="r">{count(spawn.tokens.output)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
		<p class="combined">
			Session <strong>{totalToolCalls(facts)}</strong> tool call(s) · delegated
			<strong>{Object.values(facts.delegation.totals.tool_calls).reduce((n, v) => n + v, 0)}</strong
			>
			· <strong>{combinedToolCalls(facts)}</strong> combined. Output tokens
			{count(facts.tokens.output)} + {count(facts.delegation.totals.tokens.output)} =
			<strong>{count(combinedOutputTokens(facts))}</strong>.
		</p>
		<p class="note">
			Active time is <strong>not</strong> summed: a subagent runs while the session waits on it, so those
			seconds overlap rather than add.
		</p>
		{#if facts.delegation.unjoined_spawns > 0}
			<p class="note unknown">
				{facts.delegation.unjoined_spawns} spawn(s) left no transcript to read. Their work happened and
				is <strong>not</strong> in any number here — an unknown, not a zero.
			</p>
		{/if}
		{#if facts.delegation.inline_records > 0}
			<p class="note">
				{facts.delegation.inline_records} record(s) in this transcript were subagent turns inlined by
				an older CLI. They are counted in this tier, not in the session's own totals.
			</p>
		{/if}
	</section>
{/if}

<style>
	.turns {
		list-style: none;
		margin: 12px 0 0;
		padding: 0;
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 6px 12px;
		font-size: 13.5px;
	}
	.turns li {
		display: contents;
	}
	.turns .when {
		color: var(--muted);
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
		font-size: 12px;
		white-space: nowrap;
	}
	.turns .what {
		overflow-wrap: anywhere;
	}
	.resumed {
		color: var(--muted);
	}
	.opts {
		font-size: 12px;
		color: var(--muted);
	}
	.opt {
		display: inline-block;
		border: 1px solid var(--line);
		border-radius: 999px;
		padding: 0 8px;
		margin-right: 4px;
	}
	.opt.chosen {
		border-color: var(--accent);
		color: var(--accent);
		font-weight: 600;
	}
	.unanswered {
		color: var(--ask);
	}
	.sources {
		list-style: none;
		margin: 12px 0 0;
		padding: 0;
		display: grid;
		grid-template-columns: 1fr auto auto;
		gap: 3px 10px;
		font-size: 12.5px;
		align-items: baseline;
	}
	.sources li {
		display: contents;
	}
	.sources .name {
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
		font-size: 12px;
		overflow-wrap: anywhere;
	}
	.sources .n {
		color: var(--muted);
	}
	.sources .d {
		white-space: nowrap;
	}
	.unseen {
		color: var(--ask);
	}
	.tier {
		border-collapse: collapse;
		width: 100%;
		font-size: 13px;
	}
	.tier th {
		text-align: left;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
		padding: 0 10px 6px 0;
		border-bottom: 1px solid var(--line);
	}
	.tier td {
		padding: 7px 10px 7px 0;
		border-bottom: 1px solid var(--line);
		vertical-align: top;
	}
	.tier tr:last-child td {
		border-bottom: 0;
	}
	.tier .r {
		text-align: right;
	}
	.tier em {
		color: var(--fail);
		font-style: normal;
		font-size: 12px;
		/* See Bars.svelte: without this, 3 calls and 1 failure read as "31". */
		margin-left: 0.5em;
	}
	.sub {
		display: block;
		font-size: 11px;
		color: var(--muted);
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
	}
	.combined {
		margin: 14px 0 0;
		font-size: 13.5px;
	}
	.chip.agent {
		background: var(--saidbg);
		color: var(--said);
	}
	button.open {
		font: inherit;
		font-size: 11.5px;
		margin-left: 6px;
		padding: 0 8px;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--panel);
		color: var(--muted);
		cursor: pointer;
		white-space: nowrap;
	}
	button.open:hover {
		background: var(--hover);
		color: var(--ink);
	}
	code {
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
	}
</style>
