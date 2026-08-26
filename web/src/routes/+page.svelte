<!--
  The session browser (`#/`).

  Everything the static `derived/index.html` shows, plus sort and filter, which
  is why it exists. Every figure is `sessions.json`'s, copied from the facts —
  nothing here recomputes a number, and nothing is inferred.

  Two rules from the contract are visible on this page rather than buried:
  a row with `skipped_lines > 0` is marked **partial**, because its numbers are
  incomplete; and `opaque_edits` is shown beside the file counts rather than
  folded into them, because it is an unknown and not a zero.
-->
<script lang="ts">
	import SyncLine from '$lib/components/SyncLine.svelte';
	import { loadSessions, loadSyncStatus, reportUrl } from '$lib/data.js';
	import type { SessionEntry, SyncStatus } from '$lib/contract/sessions.js';
	import { count, duration, stamp } from '$lib/format.js';
	import {
		DEFAULT_SORT,
		NO_FILTER,
		filter,
		hosts,
		projects,
		sessionHref,
		sort,
		what,
		whatIsWritten,
		type SortKey
	} from '$lib/browse.js';

	let rows = $state<SessionEntry[]>([]);
	let status = $state<SyncStatus | undefined>(undefined);
	let error = $state<string | undefined>(undefined);
	let loading = $state(true);

	let f = $state({ ...NO_FILTER });
	let s = $state({ ...DEFAULT_SORT });

	const shown = $derived(sort(filter(rows, f), s));
	const allHosts = $derived(hosts(rows));
	const allProjects = $derived(projects(rows));

	$effect(() => {
		let live = true;
		(async () => {
			try {
				const [index, sync] = await Promise.all([loadSessions(), loadSyncStatus()]);
				if (!live) return;
				rows = index.sessions;
				status = sync;
			} catch (e) {
				if (live) error = e instanceof Error ? e.message : String(e);
			} finally {
				if (live) loading = false;
			}
		})();
		return () => {
			live = false;
		};
	});

	function head(key: SortKey) {
		// A second click on the same column flips it; a new column starts at
		// its own natural direction, which is largest/newest first everywhere.
		s = s.key === key ? { key, desc: !s.desc } : { key, desc: true };
	}

	function arrow(key: SortKey): string {
		return s.key === key ? (s.desc ? '▾' : '▴') : '';
	}
</script>

<svelte:head><title>kagviz — sessions</title></svelte:head>

<h1>Sessions</h1>
<SyncLine {status} />

{#if error}
	<p class="warn">Could not read the index: {error}</p>
{:else if loading}
	<p class="note">Reading sessions.json…</p>
{:else}
	<div class="filters">
		<label>
			host
			<select bind:value={f.host}>
				<option value="">all</option>
				{#each allHosts as h (h)}<option value={h}>{h}</option>{/each}
			</select>
		</label>
		<label>
			project
			<select bind:value={f.project}>
				<option value="">all</option>
				{#each allProjects as p (p)}<option value={p}>{p}</option>{/each}
			</select>
		</label>
		<label class="grow">
			find
			<input
				type="search"
				placeholder="id, cwd, branch, or what it was about"
				bind:value={f.text}
			/>
		</label>
		<span class="note count">{shown.length} of {rows.length}</span>
	</div>

	<div class="tablewrap card">
		<table>
			<thead>
				<tr>
					<th><button onclick={() => head('host')}>host {arrow('host')}</button></th>
					<th><button onclick={() => head('project')}>project {arrow('project')}</button></th>
					<th>
						<button onclick={() => head('started')}>started (UTC) {arrow('started')}</button>
					</th>
					<th class="n"><button onclick={() => head('active')}>active {arrow('active')}</button></th
					>
					<th class="n">
						<button onclick={() => head('prompts')}>prompts {arrow('prompts')}</button>
					</th>
					<th class="n"><button onclick={() => head('tools')}>tools {arrow('tools')}</button></th>
					<th class="n"><button onclick={() => head('files')}>files {arrow('files')}</button></th>
					<th><button onclick={() => head('what')}>what {arrow('what')}</button></th>
					<th></th>
				</tr>
			</thead>
			<tbody>
				{#each shown as row (row.host + '/' + row.session_id)}
					<tr>
						<td class="mono">{row.host}</td>
						<td class="proj">
							<!-- resolve() would give /kagviz/app#/… — the directory, not the
							     shell inside it, and copyparty serves a directory as a file
							     listing. sessionHref() says why in full. -->
							<!-- eslint-disable-next-line svelte/no-navigation-without-resolve -->
							<a href={sessionHref(row)}>{row.project ?? row.cwd ?? row.session_id}</a>
							{#if row.git_branch}<span class="sub mono">{row.git_branch}</span>{/if}
							{#if row.skipped_lines > 0}
								<span
									class="chip partial"
									title="{row.skipped_lines} line(s) did not parse — every number in this row is partial"
									>partial</span
								>
							{/if}
						</td>
						<td class="when">{stamp(row.started).replace(' UTC', '')}</td>
						<td class="n">
							{duration(row.active_secs)}
							<span class="sub">{duration(row.wall_secs)} wall</span>
						</td>
						<td class="n">{count(row.user_prompts)}</td>
						<td class="n">
							{count(row.tool_calls)}
							<span class="sub">
								{#if row.tool_failures > 0}<span class="fail">{row.tool_failures} failed</span>{/if}
								{#if row.delegated_spawns > 0}
									{row.tool_failures > 0 ? ' · ' : ''}{row.delegated_spawns} agent(s)
								{/if}
							</span>
						</td>
						<td class="n">
							{count(row.files_touched)}
							<span class="sub">
								<span class="add">+{count(row.lines_added)}</span>/<span class="del"
									>−{count(row.lines_deleted)}</span
								>
								{#if row.opaque_edits > 0}
									<span
										class="opaque"
										title="{row.opaque_edits} call(s) could have changed files and left no recoverable diff — the deltas beside this are a floor, not a total"
										>+{row.opaque_edits} unseen</span
									>
								{/if}
							</span>
						</td>
						<td class="what">
							{#if whatIsWritten(row)}
								<span class="said">{what(row)}</span>
								<span class="chip said">written</span>
							{:else if what(row)}
								<span class="opened">{what(row)}</span>
							{:else}
								<span class="none">—</span>
							{/if}
						</td>
						<td class="links">
							<a href={reportUrl(row.report)} target="_blank" rel="external noreferrer">report</a>
						</td>
					</tr>
				{:else}
					<tr><td colspan="9" class="none">No session matches that filter.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
{/if}

<style>
	.filters {
		display: flex;
		gap: 14px;
		align-items: flex-end;
		flex-wrap: wrap;
		margin: 0 0 14px;
	}
	.filters label {
		display: flex;
		flex-direction: column;
		gap: 3px;
		/* Without this a flex item refuses to shrink below its content, and a
		   <select> whose longest option is a 40-character project path pushed
		   the whole page wider than a phone. Only the table may scroll
		   sideways; the page must not. */
		min-width: 0;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
	}
	.filters .grow {
		flex: 1 1 240px;
	}
	.filters select,
	.filters input {
		font: inherit;
		font-size: 14px;
		text-transform: none;
		letter-spacing: 0;
		color: var(--ink);
		background: var(--panel);
		border: 1px solid var(--line);
		border-radius: 6px;
		padding: 5px 8px;
		max-width: 100%;
	}
	.filters select {
		width: 22ch;
	}
	.filters .count {
		margin: 0 0 6px;
	}
	.tablewrap {
		padding: 0;
		overflow-x: auto;
	}
	table {
		border-collapse: collapse;
		width: 100%;
		font-size: 13.5px;
	}
	th {
		text-align: left;
		border-bottom: 1px solid var(--line);
		padding: 0;
		position: sticky;
		top: 0;
		background: var(--sticky);
		z-index: 1;
	}
	th button {
		font: inherit;
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--muted);
		font-weight: 600;
		background: none;
		border: 0;
		padding: 10px 10px;
		width: 100%;
		text-align: inherit;
		cursor: pointer;
	}
	th button:hover {
		color: var(--ink);
	}
	th.n,
	td.n {
		text-align: right;
	}
	td {
		padding: 8px 10px;
		border-bottom: 1px solid var(--line);
		vertical-align: top;
	}
	tr:last-child td {
		border-bottom: 0;
	}
	tbody tr:hover {
		background: var(--hover);
	}
	.mono {
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
	}
	.proj a {
		font-weight: 600;
		text-decoration: none;
	}
	.proj a:hover {
		text-decoration: underline;
	}
	.when {
		white-space: nowrap;
		color: var(--muted);
	}
	.sub {
		display: block;
		font-size: 11.5px;
		color: var(--muted);
		white-space: nowrap;
	}
	.fail {
		color: var(--fail);
	}
	.opaque {
		color: var(--ask);
	}
	/* `max-width` on a table cell is advisory under `table-layout: auto`, so
	   the longest prompt preview widened the column until the columns after it
	   were pushed off the page. The cap has to sit on a block *inside* the
	   cell, where it is honoured. */
	/* Every column here is content-sized, so the two that can run long are
	   capped and the rest are left alone: nine columns of unbounded text do
	   not fit 1400px, and the `report` link is the one that falls off the end.
	   The caps sit on a block *inside* the cell — `max-width` on a `td` is
	   advisory under `table-layout: auto`. The floor matters as much as the
	   cap: `overflow-wrap` drives min-content down to one character, and
	   without a `min-width` the column collapses to a few glyphs and every row
	   grows five lines tall. Below the sum of the caps the wrapper scrolls,
	   which is the honest answer for a nine-column table on a phone. */
	.what > span {
		display: block;
		min-width: 20ch;
		max-width: 32ch;
		overflow-wrap: break-word;
	}
	.proj > a {
		display: inline-block;
		max-width: 20ch;
		overflow-wrap: break-word;
	}
	.what .said {
		font-style: italic;
		font-family: ui-serif, Georgia, 'Times New Roman', serif;
		color: var(--ink);
	}
	.what .opened {
		color: var(--muted);
	}
	.none {
		color: var(--muted);
	}
	.chip.partial {
		background: var(--warnbg);
		color: var(--ask);
		margin-left: 6px;
	}
	.links {
		white-space: nowrap;
		font-size: 12px;
	}
</style>
