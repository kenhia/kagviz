<!--
  What is behind the selected piece of the timeline.

  The rows come from `segment.ts`, which merges two documents: the events
  (turns and tool calls) and the facts' `user_involvement` (prompts and
  questions, which the events deliberately do not repeat). This is the only
  place in the app that reads both for one answer.

  The counts are rendered from **both tiers**, because #1639 asks for that and
  because it is the only way to keep the panel honest:

  - a **disagreement** is the two documents counting the same thing and
    differing — a defect in this app, and it is shown as a warning rather than
    rounded away;
  - a **note** is the two counting *different* things. There is exactly one:
    a failure is counted where its result came back and a call is drawn where
    it was made, so a call whose result crossed the boundary lands on one side
    and is drawn on the other. Sprint 012 measured that over the corpus and
    corrected the contract, which had asserted an equality that does not hold.

  Since sprint 015 a tool row opens a second time, into the calls document:
  what the call actually said. That is the first thing this app shows which
  was never counted — it is raw session content — so it is fenced and
  labelled as such, and the rules the rest of the panel follows hold inside
  it. Absent is absent: an interrupted call says so rather than drawing an
  empty box, an offloaded result says what it is a preview *of*, and a result
  that was an image says that instead of showing nothing.
-->
<script lang="ts">
	import type { Facts, ToolClass } from '$lib/contract/facts.js';
	import type { EventsDocument, ToolEvent } from '$lib/contract/events.js';
	import { byId, type CallsDocument } from '$lib/contract/calls.js';
	import { callView, clip, type CallsState } from '$lib/calltext.js';
	import { segment, type Row, type Selection } from '$lib/segment.js';
	import { bytes, count, duration, signed, stamp } from '$lib/format.js';
	import { phaseLabel } from '$lib/contract/derived.js';
	import { SvelteSet } from 'svelte/reactivity';

	let {
		facts,
		events,
		selection,
		onclear,
		calls,
		callsState = 'unasked',
		callsError,
		onopencalls
	}: {
		facts: Facts;
		events: EventsDocument;
		selection: Selection;
		onclear: () => void;
		/** The payload tier, once the page has fetched it. */
		calls?: CallsDocument;
		/**
		 * Where the fetch has got to.
		 *
		 * `absent` is a real answer and not a failure: `derive` writes call
		 * text only when asked, so most trees carry none, and a reader who
		 * clicks deserves to be told that rather than shown an empty box.
		 */
		callsState?: CallsState;
		callsError?: string;
		/** Ask the page to fetch it. Lazy on purpose: never with the events. */
		onopencalls?: () => void;
	} = $props();

	const seg = $derived(segment(facts, events, selection));

	/**
	 * A tool class wears the colour of the phase kind its share produces.
	 *
	 * `classify_phase` in `summary.rs` is the mapping: delegate→delegating,
	 * edit→implementing, org→filing, read→exploring, run→running. `ask` and
	 * `other` have no rule of their own there — an ask-heavy phase classifies
	 * as `mixed` — so `ask` borrows the discussing colour, which is a
	 * presentation choice and not a claim about the facts.
	 */
	const CLASS_KIND: Record<ToolClass, string> = {
		read: 'exploring',
		edit: 'implementing',
		run: 'running',
		org: 'filing',
		delegate: 'delegating',
		ask: 'discussing',
		other: 'mixed'
	};

	const title = $derived.by(() => {
		const sel = selection;
		if (sel.kind === 'phase') {
			const p = facts.phases[sel.phase];
			return p ? `phase ${sel.phase + 1} of ${facts.phases.length} — ${p.kind}` : 'phase';
		}
		if (sel.kind === 'spawn') {
			const sp = facts.delegation.spawns[sel.spawn];
			const what = sp?.subagent_type ?? 'agent';
			return `delegated — ${what} ${sel.spawn + 1} of ${facts.delegation.spawns.length}`;
		}
		return `${duration(sel.to - sel.from)} of stretch ${sel.span + 1}`;
	});

	const written = $derived.by(() => {
		const sel = selection;
		return sel.kind === 'phase' ? phaseLabel(facts, sel.phase) : undefined;
	});

	/** A subagent's tier is described by the facts, not by anything on screen. */
	const spawn = $derived.by(() => {
		const sel = selection;
		return sel.kind === 'spawn' ? facts.delegation.spawns[sel.spawn] : undefined;
	});

	const when = $derived(
		seg.range
			? `${stamp(new Date(seg.range.fromMs).toISOString())} → ${new Date(seg.range.toMs).toISOString().slice(11, 16)}`
			: '—'
	);

	/** `result_at − at`, or the absence said in words. Never a zero. */
	function took(t: ToolEvent): string {
		const a = Date.parse(t.at ?? '');
		const b = Date.parse(t.result_at ?? '');
		if (Number.isNaN(a) || Number.isNaN(b)) return 'no result — interrupted or still running';
		return duration(Math.max(0, Math.round((b - a) / 1000)));
	}

	const open = new SvelteSet<string>();

	function toggle(key: string) {
		if (!open.delete(key)) open.add(key);
	}

	/** Rows whose call text is open, and those expanded past the head. */
	const openText = new SvelteSet<string>();
	const openFull = new SvelteSet<string>();

	const called = $derived(calls ? byId(calls) : undefined);

	function toggleText(key: string) {
		if (!openText.delete(key)) {
			openText.add(key);
			// One fetch per session, asked for by the first row that wants it —
			// and nothing at all for a reader who never opens a call.
			if (callsState === 'unasked') onopencalls?.();
		}
	}

	/**
	 * What an expanded row shows, decided in `calltext.ts` rather than here.
	 *
	 * The rules it applies are the ones the contract cares about — absent is
	 * not empty, a non-text result is not an empty one, a preview is not the
	 * output — and a rule that lives in markup is a rule nothing tests.
	 */
	function view(toolUseId: string) {
		return callView(callsState, called?.get(toolUseId), toolUseId, callsError);
	}

	function key(r: Row, i: number): string {
		return `${i}:${r.kind}`;
	}
</script>

<section class="card seg">
	<div class="head">
		<h2>{title}</h2>
		<button onclick={onclear} aria-label="clear the selection">clear</button>
	</div>
	<p class="when">{when}</p>
	{#if spawn}
		<p class="note">
			{spawn.description ?? 'no description on the Agent call'}{spawn.model
				? ` · ${spawn.model}`
				: ''} · {duration(spawn.active_secs)} active. A spawn's events carry no phase — phases cut the
			parent's timeline{spawn.sidecar ? ', and these came from its own sidecar transcript' : ''}.
		</p>
	{/if}
	{#if written}
		<p class="said">
			<em>written</em>
			{written}
		</p>
	{/if}

	<ul class="tally">
		<li><b>{count(seg.counts.turns)}</b> turn(s)</li>
		<li><b>{count(seg.counts.toolCalls)}</b> tool call(s)</li>
		<li>
			<b class:bad={seg.counts.failures > 0}>{count(seg.counts.failures)}</b> failed
		</li>
		<li><b>{count(seg.counts.outputTokens)}</b> out</li>
		{#if seg.counts.files > 0}
			<li>
				<b>{count(seg.counts.files)}</b> file(s)
				<span class="add">{signed(seg.counts.linesAdded)}</span>/<span class="del"
					>{signed(-seg.counts.linesDeleted)}</span
				>
			</li>
		{/if}
		{#if seg.counts.opaque > 0}
			<li><b>{count(seg.counts.opaque)}</b> opaque</li>
		{/if}
		{#if seg.counts.prompts > 0}
			<li><b>{count(seg.counts.prompts)}</b> prompt(s)</li>
		{/if}
		{#if seg.counts.questions > 0}
			<li><b>{count(seg.counts.questions)}</b> question(s)</li>
		{/if}
	</ul>

	{#if seg.facts}
		<p class="tiers">
			The facts count <b>{count(seg.facts.toolCalls)}</b> tool call(s),
			<b>{count(seg.facts.failures)}</b> failure(s), <b>{count(seg.facts.outputTokens)}</b> output
			token(s) and <b>{count(seg.facts.records)}</b> record(s) here. Records are the one count the events
			do not reproduce — they include system and snapshot records, which carry nothing worth an event.
		</p>
	{/if}
	{#each seg.disagreements as d, i (i)}
		<p class="warn">The two documents disagree, and that is a defect in this page: {d}</p>
	{/each}
	{#each seg.notes as n, i (i)}
		<p class="note">{n}</p>
	{/each}

	{#if seg.rows.length === 0}
		<p class="note">Nothing happened in this window — no turn, no call, nothing said.</p>
	{:else}
		<ol class="rows">
			{#each seg.rows as row, i (key(row, i))}
				{#if row.kind === 'said'}
					<li class="row said-row">
						<span class="t">{row.at ? row.at.slice(11, 19) : '—'}</span>
						<span class="body">
							{#if row.item.kind === 'prompt'}
								<b class="who user">user</b>
								{row.item.preview}{row.item.truncated ? '…' : ''}
								{#if row.item.attachments > 0}
									<span class="dim">· {row.item.attachments} attachment(s)</span>
								{/if}
							{:else}
								<b class="who ask">asked</b>
								{row.item.question}
								<span class="dim">
									· {row.item.options.length} option(s) ·
									{#if row.item.chosen}chose “{row.item.chosen}”{:else}no answer in the transcript{/if}
								</span>
							{/if}
						</span>
					</li>
				{:else if row.kind === 'turn'}
					<li class="row turn-row">
						<span class="t">{row.at ? row.at.slice(11, 19) : '—'}</span>
						<span class="body">
							<b class="who model">{row.turn.model ?? 'assistant'}</b>
							{#if row.turn.tokens}
								<span class="dim">
									{count(row.turn.tokens.output)} out
									{#if row.turn.tokens.thinking > 0}· {count(row.turn.tokens.thinking)} thinking{/if}
									· {count(row.turn.tokens.input)} in
								</span>
							{:else}
								<span class="dim">no usage on the record</span>
							{/if}
							{#if row.turn.tools > 0}
								<span class="dim">· {row.turn.tools} call(s)</span>
							{/if}
						</span>
					</li>
					{#each row.tools as t, ti (ti)}
						{@render tool(t, `${i}-${ti}`)}
					{/each}
				{:else}
					{@render tool(row.tool, `${i}`)}
				{/if}
			{/each}
		</ol>
	{/if}
</section>

{#snippet tool(t: ToolEvent, id: string)}
	<li class="row tool-row" class:failed={t.failed}>
		<span class="t">{t.at ? t.at.slice(11, 19) : '—'}</span>
		<span class="body">
			<span class="cls k-{CLASS_KIND[t.class]}" title={t.class}></span>
			<b class="tool">{t.tool}</b>
			<span class="dim">
				{took(t)}
				{#if t.input_bytes !== undefined}· {bytes(t.input_bytes)} in{/if}
				{#if t.result_bytes !== undefined}· {bytes(t.result_bytes)} out{/if}
			</span>
			{#if t.failed}<span class="tag bad">failed</span>{/if}
			{#if t.opaque}<span
					class="tag"
					title="a shell call that could have written and left no readable diff">opaque</span
				>{/if}
			{#if t.files.length > 0}
				<button class="more" onclick={() => toggle(id)} aria-expanded={open.has(id)}>
					{t.files.length} file(s)
					{#if t.lines_added !== undefined && t.lines_deleted !== undefined}
						<span class="add">{signed(t.lines_added)}</span>/<span class="del"
							>{signed(-t.lines_deleted)}</span
						>
					{:else}
						<span class="dim">— no diff to read</span>
					{/if}
				</button>
			{/if}
			{#if t.id !== undefined}
				<button class="more" onclick={() => toggleText(id)} aria-expanded={openText.has(id)}>
					{openText.has(id) ? 'hide' : 'read'} the call
				</button>
			{/if}
		</span>
		{#if t.files.length > 0 && open.has(id)}
			<ul class="files">
				{#each t.files as f (f)}
					<li>{f}</li>
				{/each}
			</ul>
		{/if}
		{#if openText.has(id) && t.id !== undefined}
			{@render callText(t.id, id)}
		{/if}
	</li>
{/snippet}

<!--
  What the call said.

  Everything else this app shows was counted; this was not. It is the
  transcript's own text — command output, file contents, whatever was pasted
  in — so it is fenced, labelled, and rendered as **text**: monospace,
  wrapped, no markdown, no HTML, no highlighting. A tool result is arbitrary
  bytes and deciding what it "is" is a decision that goes wrong on a shared
  screen.
-->
{#snippet callText(toolUseId: string, id: string)}
	{@const v = view(toolUseId)}
	<div class="calltext">
		{#if v.kind === 'absent'}
			<!-- Not a failure: the default state of every derived tree. The reader
			     is told which knob changes it rather than left wondering what
			     broke. -->
			<p class="dim">
				This tree carries no call text. It is the one document kagviz does not derive by default,
				because it is the transcript's own words rather than something counted from them —
				<code>kagviz derive --calls</code> writes it.
			</p>
		{:else if v.kind === 'error'}
			<p class="warn">Could not read this session's call text: {v.message}</p>
		{:else if v.kind === 'loading'}
			<p class="dim">Reading the call text…</p>
		{:else if v.kind === 'unjoined'}
			<p class="warn">
				No entry for <code>{v.id}</code> in this session's calls document — the two documents have drifted.
			</p>
		{:else}
			<p class="raw">Raw session content, shown as written. Nothing counted or checked it.</p>

			<h4>sent</h4>
			{#if v.input === undefined}
				<p class="dim">No input recorded on this call.</p>
			{:else}
				{@const c = clip(v.input, openFull.has(`${id}:in`))}
				<pre>{c.shown}</pre>
				{#if c.hidden}
					<!-- No byte figure here, deliberately. The row above already
					     shows `input_bytes`, which the contract defines as the
					     *canonical* compact serialization — and what is on screen
					     is the formatted one, which is larger. Two numbers for one
					     input, differing, is the disagreement this panel warns
					     about everywhere else; the result's button below can carry
					     its size because there the two are the same text. -->
					<button class="more" onclick={() => openFull.add(`${id}:in`)}> show the rest </button>
				{/if}
			{/if}

			<h4>came back</h4>
			{#if v.persisted}
				<p class="note">
					This is the preview the model was handed, not the output. The harness judged the real
					output too large for the context{v.persisted.bytes !== undefined
						? ` — ${bytes(v.persisted.bytes)} of it`
						: ''} and saved it beside the transcript; that file is not served.
				</p>
			{/if}
			{#if v.result.kind === 'interrupted'}
				<!-- Absent is not empty, and this is the line that keeps it so. -->
				<p class="dim">
					No result — the call was interrupted, or was still running when the transcript ended.
				</p>
			{:else if v.result.kind === 'empty'}
				<p class="dim">A result came back carrying no text.</p>
			{:else}
				{@const c = clip(v.result.text, openFull.has(`${id}:out`))}
				<pre>{c.shown}</pre>
				{#if c.hidden}
					<button class="more" onclick={() => openFull.add(`${id}:out`)}>
						show all {bytes(c.bytes)}
					</button>
				{/if}
			{/if}
			{#if v.blocks.length > 0}
				<!-- Why an empty result here is not an empty result: 4,672 of the
				     45,394 calls in the 015 corpus sweep came back with one. -->
				<p class="note">
					The result also carried {v.blocks.length}
					{v.blocks.join(', ')} block(s), which carry no text and are not in this document.
				</p>
			{/if}
		{/if}
	</div>
{/snippet}

<style>
	.seg {
		scroll-margin-top: 12px;
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: 12px;
	}
	.head h2 {
		margin: 0;
		text-transform: none;
		letter-spacing: 0;
		font-size: 14px;
		color: var(--ink);
	}
	.head button {
		margin-left: auto;
		font: inherit;
		font-size: 12px;
		padding: 1px 9px;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--panel);
		color: var(--muted);
		cursor: pointer;
	}
	.when {
		margin: 2px 0 10px;
		font-size: 12px;
		color: var(--muted);
	}
	.said {
		margin: 0 0 10px;
		font-size: 13px;
		background: var(--saidbg);
		color: var(--said);
		border-radius: 6px;
		padding: 5px 9px;
	}
	.said em {
		font-style: normal;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		margin-right: 6px;
	}
	ul.tally {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 18px;
		list-style: none;
		margin: 0 0 10px;
		padding: 0;
		font-size: 12.5px;
		color: var(--muted);
	}
	ul.tally b {
		color: var(--ink);
		font-variant-numeric: tabular-nums;
	}
	ul.tally b.bad {
		color: var(--fail);
	}
	.tiers,
	.note {
		margin: 0 0 8px;
		font-size: 11.5px;
		line-height: 1.55;
		color: var(--muted);
	}
	.tiers b {
		color: var(--ink);
		font-variant-numeric: tabular-nums;
	}
	.warn {
		margin: 0 0 8px;
		font-size: 12px;
		background: var(--warnbg);
		border-radius: 6px;
		padding: 6px 9px;
	}
	ol.rows {
		list-style: none;
		margin: 0;
		padding: 0;
		border-top: 1px solid var(--line);
		max-height: 620px;
		overflow-y: auto;
	}
	li.row {
		display: grid;
		grid-template-columns: 66px 1fr;
		gap: 8px;
		padding: 4px 2px;
		border-bottom: 1px solid var(--line);
		font-size: 12.5px;
		line-height: 1.5;
	}
	li.row:hover {
		background: var(--hover);
	}
	li.tool-row {
		padding-left: 16px;
		grid-template-columns: 50px 1fr;
	}
	li.tool-row.failed {
		box-shadow: inset 2px 0 0 var(--fail);
	}
	.t {
		color: var(--muted);
		font-variant-numeric: tabular-nums;
		font-size: 11px;
		white-space: nowrap;
	}
	.body {
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.who {
		font-weight: 600;
		margin-right: 5px;
	}
	.who.user {
		color: var(--user);
	}
	.who.ask {
		color: var(--ask);
	}
	.who.model {
		color: var(--muted);
		font-weight: 500;
	}
	b.tool {
		margin-right: 5px;
	}
	.dim {
		color: var(--muted);
		font-size: 11.5px;
	}
	.cls {
		display: inline-block;
		width: 8px;
		height: 8px;
		border-radius: 2px;
		margin-right: 6px;
		vertical-align: 0;
	}
	.k-exploring {
		background: var(--ph-exploring);
	}
	.k-implementing {
		background: var(--ph-implementing);
	}
	.k-running {
		background: var(--ph-running);
	}
	.k-filing {
		background: var(--ph-filing);
	}
	.k-delegating {
		background: var(--ph-delegating);
	}
	.k-discussing {
		background: var(--ph-discussing);
	}
	.k-mixed {
		background: var(--ph-mixed);
	}
	.tag {
		display: inline-block;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		border-radius: 3px;
		padding: 0 5px;
		margin-left: 5px;
		background: var(--chip);
		color: var(--muted);
	}
	.tag.bad {
		background: var(--fail);
		color: var(--panel);
	}
	.more {
		font: inherit;
		font-size: 11.5px;
		margin-left: 6px;
		padding: 0 6px;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--panel);
		color: var(--muted);
		cursor: pointer;
	}
	.add {
		color: var(--add);
	}
	.del {
		color: var(--del);
	}
	.calltext {
		grid-column: 2;
		margin: 6px 0 2px;
		padding: 8px 10px;
		border-left: 2px solid var(--rule, #d0d0d0);
		background: var(--sunken, rgba(127, 127, 127, 0.06));
	}
	.calltext h4 {
		margin: 8px 0 3px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		opacity: 0.65;
	}
	.calltext h4:first-of-type {
		margin-top: 2px;
	}
	.calltext p {
		margin: 3px 0;
		font-size: 0.78rem;
	}
	.calltext p.raw {
		margin: 0 0 6px;
		font-size: 0.72rem;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		opacity: 0.7;
	}
	.calltext p.note {
		font-style: italic;
		opacity: 0.8;
	}
	.calltext pre {
		margin: 0;
		padding: 6px 8px;
		max-height: 22em;
		overflow: auto;
		font-size: 0.76rem;
		line-height: 1.45;
		/* A tool result is arbitrary bytes: wrap it, never let it push the
		   page sideways, and never let a long token escape the box. */
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		background: var(--code-bg, rgba(127, 127, 127, 0.1));
	}
	ul.files {
		grid-column: 2;
		list-style: none;
		margin: 3px 0 2px;
		padding: 0;
		font-size: 11.5px;
		color: var(--muted);
		overflow-wrap: anywhere;
	}
</style>
