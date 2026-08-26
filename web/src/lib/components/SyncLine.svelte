<!--
  Which hosts the last sync reached.

  "not reached" must stay visible: a partial sync being mistaken for "nothing
  new" is the exact failure `sync-status.json` exists to prevent, so an
  unreachable host is rendered in the place a count would be, not omitted. A
  tree with no status file says that too, rather than nothing.
-->
<script lang="ts">
	import type { SyncStatus } from '$lib/contract/sessions.js';
	import { stamp } from '$lib/format.js';

	let { status }: { status: SyncStatus | undefined } = $props();

	const entries = $derived(
		Object.entries(status?.hosts ?? {}).sort(([a], [b]) => a.localeCompare(b))
	);
</script>

<p class="note sync">
	{#if !status}
		<span class="unreached">no sync status</span> — this tree carries no
		<code>sync-status.json</code>, so which hosts the last sync reached is unknown.
	{:else}
		last sync {stamp(status.ran_at)} —
		{#each entries as [host, h], i (host)}
			{#if i > 0}<span class="sep">·</span>{/if}
			<span class:unreached={h.status !== 'ok'}>
				{host}
				{#if h.status === 'ok'}
					{h.transferred ?? 0} file(s)
				{:else}
					{h.status ?? 'unknown'}{#if h.note}<span class="why"> ({h.note})</span>{/if}
				{/if}
			</span>
		{:else}
			no hosts recorded
		{/each}
	{/if}
</p>

<style>
	.sync {
		margin: 0 0 14px;
	}
	.sep {
		margin: 0 6px;
		opacity: 0.5;
	}
	.unreached {
		color: var(--ask);
		font-weight: 600;
	}
	.why {
		font-weight: 400;
	}
	code {
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
	}
</style>
