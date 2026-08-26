<!--
  The time strip, drawn once as SVG.

  Static in this sprint on purpose: no pan, no zoom, no click. Those are part
  2 (#1591, #1639), and they will re-bucket from the events document rather
  than from here — `bucket_secs` is the session's choice, and this draws the
  series at exactly that resolution.

  Each span gets its own SVG scaled to its bucket count, so an idle break is a
  fixed-width mark between two proportional stretches rather than a
  proportional stretch of nothing. `preserveAspectRatio="none"` stretches the
  horizontal only — the viewBox height matches the CSS height, so nothing is
  distorted vertically and there is no text inside to smear.
-->
<script lang="ts">
	import type { Facts } from '$lib/contract/facts.js';
	import { duration } from '$lib/format.js';
	import { layout } from '$lib/strip.js';

	let { facts }: { facts: Facts } = $props();

	const strip = $derived(layout(facts));

	const BANDS_H = 16;
	const MARKS_H = 9;
	const BARS_H = 95;
	const H = BANDS_H + MARKS_H + BARS_H;
</script>

<div class="strip {strip.density}">
	{#each strip.spans as span, si (si)}
		{#if span.idleBefore > 0}
			<div class="gap" title="{duration(span.idleBefore)} idle">
				{#if strip.labelBreaks}<span>{duration(span.idleBefore)}</span>{/if}
			</div>
		{/if}
		<div class="span" style="flex-grow:{Math.max(1, span.columns.length)}">
			<svg
				viewBox="0 0 {Math.max(1, span.columns.length)} {H}"
				preserveAspectRatio="none"
				style="height:{H}px"
				role="img"
				aria-label="{duration(span.secs)} of work in {span.columns.length} bucket(s)"
			>
				{#each span.bands as band, bi (bi)}
					<rect
						x={band.from}
						y="0"
						width={Math.max(0.0001, band.to - band.from)}
						height={BANDS_H}
						fill="var(--ph-{band.kind})"
					>
						<title>{band.tip}{band.written ? ` — written: ${band.written}` : ''}</title>
					</rect>
				{/each}
				{#each span.columns as col, ci (ci)}
					{#if col.mark}
						<rect
							x={ci + 0.1}
							y={BANDS_H + 1}
							width="0.8"
							height={MARKS_H - 3}
							fill={col.mark.kind === 'question' ? 'var(--ask)' : 'var(--user)'}
						>
							<title>{col.tip}</title>
						</rect>
					{/if}
					{#if col.pct > 0}
						<rect
							x={ci + 0.05}
							y={H - (BARS_H * col.pct) / 100}
							width="0.9"
							height={(BARS_H * col.pct) / 100}
							fill={col.failed ? 'var(--fail)' : 'var(--bar)'}
						>
							<title>{col.tip}</title>
						</rect>
					{:else}
						<rect x={ci} y={H - 1} width="1" height="0.0001" fill="none">
							<title>{col.tip}</title>
						</rect>
					{/if}
				{/each}
			</svg>
			<div class="axis">
				<span>{span.started.slice(11, 16)}</span>
				<span>{duration(span.secs)}</span>
			</div>
		</div>
	{/each}
</div>

<p class="legend">
	<span class="key bar"></span>work
	<span class="key barfail"></span>tool failure
	<span class="key mkuser"></span>user prompt
	<span class="key mkask"></span>question asked
	<span class="key gapkey"></span>idle, collapsed
</p>

<style>
	.strip {
		display: flex;
		align-items: stretch;
		border: 1px solid var(--line);
		border-radius: 8px;
		overflow: hidden;
		background: var(--bg);
	}
	.span {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex-shrink: 1;
		flex-basis: 0;
	}
	svg {
		display: block;
		width: 100%;
	}
	/* The axis text is gated on the span's own rendered width: a one-column
	   span showing half a timestamp, two hundred times, is a row of junk. */
	.axis {
		display: flex;
		justify-content: space-between;
		font-size: 10.5px;
		line-height: 15px;
		min-height: 22px;
		color: var(--muted);
		padding: 4px 3px 2px;
		border-top: 1px solid var(--line);
		gap: 6px;
		white-space: nowrap;
		overflow: hidden;
		container-type: inline-size;
	}
	.axis span {
		display: none;
	}
	.axis span:last-child {
		margin-left: auto;
	}
	@container (min-width: 44px) {
		.axis span:last-child {
			display: inline;
		}
	}
	@container (min-width: 136px) {
		.axis span:first-child {
			display: inline;
		}
	}
	/* Three densities, the report's own thresholds. Sixty 6px breaks is about
	   350px — a quarter of a typical strip, and the most idle should ever take
	   on a panel whose job is collapsing it. Past that they narrow again and
	   drop their dashed edges, because at that density the edges *are* the
	   break. Narrowing further is not the answer: past a few hundred stretches
	   the strip stops being legible at all, and the fix there is the zoom that
	   part 2 brings, not shaving pixels here. */
	.strip.roomy .gap {
		flex: 0 0 32px;
	}
	.strip.dense .gap {
		flex: 0 0 6px;
	}
	.strip.packed .gap {
		flex: 0 0 3px;
		border-left-width: 0;
		border-right-width: 0;
	}
	.gap {
		display: flex;
		align-items: center;
		justify-content: center;
		background: repeating-linear-gradient(135deg, transparent 0 4px, var(--line) 4px 5px);
		border-left: 1px dashed var(--line);
		border-right: 1px dashed var(--line);
	}
	.legend {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 16px;
		align-items: center;
		margin: 10px 0 0;
		font-size: 11.5px;
		color: var(--muted);
	}
	.key {
		display: inline-block;
		width: 11px;
		height: 11px;
		border-radius: 2px;
		margin-right: 5px;
		vertical-align: -1px;
	}
	.key.bar {
		background: var(--bar);
	}
	.key.barfail {
		background: var(--fail);
	}
	.key.mkuser {
		background: var(--user);
	}
	.key.mkask {
		background: var(--ask);
	}
	.key.gapkey {
		background: repeating-linear-gradient(135deg, transparent 0 3px, var(--line) 3px 4px);
		border: 1px solid var(--line);
	}
	.gap span {
		font-size: 10px;
		color: var(--muted);
		background: var(--panel);
		padding: 1px 3px;
		border-radius: 3px;
		transform: rotate(-90deg);
		white-space: nowrap;
	}
</style>
