<!--
  A ranked bar list: name, proportional track, count. Used for the tool mix
  (where the failed share is drawn in the failure colour on the same track) and
  for the phase rollup (where the fill takes the phase kind's colour).
-->
<script lang="ts">
	interface Row {
		name: string;
		/** Main fill, as a share of the peak row. */
		pct: number;
		/** A second fill drawn after the first — failures, on the tool mix. */
		failPct?: number;
		/** A phase kind, when the fill should take that kind's colour. */
		kind?: string;
		note: string;
		emphasis?: string;
	}

	let { rows }: { rows: Row[] } = $props();
</script>

<ul class="bars">
	{#each rows as row (row.name)}
		<li>
			<span class="name">{row.name}</span>
			<span class="track">
				<span
					class="fill"
					class:kinded={!!row.kind}
					style="width:{row.pct.toFixed(3)}%; {row.kind ? `background:var(--ph-${row.kind})` : ''}"
				></span>
				{#if row.failPct}
					<span class="fill fail" style="width:{row.failPct.toFixed(3)}%"></span>
				{/if}
			</span>
			<span class="n">
				<span class="v">{row.note}</span>{#if row.emphasis}<em>{row.emphasis}</em>{/if}
			</span>
		</li>
	{/each}
</ul>

<style>
	.bars {
		list-style: none;
		margin: 0;
		padding: 0;
		display: grid;
		grid-template-columns: minmax(6ch, auto) 1fr auto;
		gap: 4px 10px;
		align-items: center;
		font-size: 13px;
	}
	li {
		display: contents;
	}
	.name {
		font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
		font-size: 12px;
		overflow-wrap: anywhere;
	}
	.track {
		display: flex;
		height: 10px;
		background: var(--chip);
		border-radius: 3px;
		overflow: hidden;
		min-width: 40px;
	}
	.fill {
		background: var(--bar);
		height: 100%;
	}
	.fill.fail {
		background: var(--fail);
	}
	.n {
		font-size: 12px;
		color: var(--muted);
		white-space: nowrap;
	}
	.n em {
		color: var(--fail);
		font-style: normal;
		/* A gap that survives whitespace collapsing: "4" and "1 failed" run
		   together into "41 failed" otherwise, which is a wrong number on the
		   page rather than a cosmetic one. */
		margin-left: 0.5em;
	}
</style>
