<!--
  The timeline — pan, zoom, and the click that opens a segment.

  Supersedes sprint 011's `Strip.svelte`, which drew the series once at the
  session's own `bucket_secs`. The geometry is all in `timeline.ts` and pure;
  what is here is the interaction and nothing else.

  Three things worth knowing before changing it:

  **One SVG, sized to the viewport, not to the track.** At full zoom a long
  session's track is a million pixels wide. The `<svg>` is positioned at the
  scroll offset and given a `viewBox` in *track* coordinates, so its children
  can use absolute track x while only the visible slice is ever in the DOM.

  **One click handler, not one per rect.** Hit-testing goes through
  `locate()`. A listener per column would mean tens of thousands of them, and
  making them focusable would be worse for a keyboard than what is here: the
  container takes focus once, and arrow keys move the selection.

  **The bar means different things at different zooms, and the caption says
  which.** Above `bucket_secs` a column is whole facts buckets and the bar
  counts records; below it there is nothing finer in the facts and the column
  is re-bucketed from the events, counting turns and tool calls. Those are not
  the same number — `records` counts `system` and snapshot records too — so
  they are never drawn as if they were.
-->
<script lang="ts">
	import type { Facts } from '$lib/contract/facts.js';
	import type { EventsDocument } from '$lib/contract/events.js';
	import type { Selection } from '$lib/segment.js';
	import { count, duration, stamp } from '$lib/format.js';
	import {
		MAX_PX_PER_SEC,
		bands as layoutBands,
		breakWidth,
		columnAt,
		columns as layoutColumns,
		fitPxPerSec,
		locate,
		marks as layoutMarks,
		place,
		resolution,
		ticks as layoutTicks,
		track as layoutTrack,
		visible
	} from '$lib/timeline.js';

	let {
		facts,
		events,
		selection = $bindable(),
		frame = $bindable()
	}: {
		facts: Facts;
		events?: EventsDocument;
		selection?: Selection;
		/** Set by the page to ask the timeline to frame a phase or a span. */
		frame?: Selection;
	} = $props();

	const BANDS_H = 16;
	const MARKS_H = 9;
	const BARS_H = 95;
	const AXIS_H = 16;
	const H = BANDS_H + MARKS_H + BARS_H + AXIS_H;

	let viewport = $state<HTMLDivElement | undefined>(undefined);
	let width = $state(900);
	let scroll = $state(0);
	let pxPerSec = $state(0);

	const fit = $derived(fitPxPerSec(facts, width));
	const scale = $derived(pxPerSec > 0 ? Math.min(Math.max(pxPerSec, fit), MAX_PX_PER_SEC) : fit);
	const zoomed = $derived(scale > fit * 1.0001);
	// The fit view is always the facts' own resolution, even once the events
	// are here. It is the report's strip, it is what the page draws before the
	// big document lands, and a panel that changes what its bars count on its
	// own — the moment a fetch completes — is the thing the caption exists to
	// prevent. Going finer is something the reader does, by zooming.
	const res = $derived(
		resolution(scale, facts.activity.bucket_secs, zoomed && events !== undefined)
	);
	const track = $derived(
		layoutTrack(facts, scale, breakWidth(facts, scale, width, res.secs * scale))
	);
	const cols = $derived(layoutColumns(facts, events, track, res));
	const bands = $derived(layoutBands(facts, track));
	const marks = $derived(layoutMarks(facts, track));

	// A margin either side, so a fast drag does not outrun the render.
	const from = $derived(Math.max(0, scroll - width));
	const to = $derived(scroll + width * 2);
	const seen = $derived({
		cols: cols ? visible(cols.items, from, to) : [],
		bands: visible(bands.items, from, to),
		marks: visible(marks, from, to),
		ticks: layoutTicks(track, from, to)
	});

	const bucket = $derived(facts.activity.bucket_secs);

	/** The bands and the window each get a highlight; narrowed here, once. */
	const selWindow = $derived.by(() => {
		const sel = selection;
		return sel?.kind === 'window' ? sel : undefined;
	});
	const selBand = $derived.by(() => {
		const sel = selection;
		return sel?.kind === 'phase' ? bands.items.find((b) => b.phase === sel.phase) : undefined;
	});

	/** What is on screen, in words — the "where am I" the breadcrumb needs. */
	const here = $derived.by(() => {
		const a = locate(track, scroll);
		const b = locate(track, scroll + width);
		if (!a || !b) return '';
		const spans = b.span - a.span;
		const one = facts.activity.spans[a.span];
		const at = new Date((Date.parse(one.started) || 0) + a.secs * 1000).toISOString();
		const shown = `${duration(Math.max(1, Math.round(width / scale)))} of work on screen`;
		return spans > 0
			? `${stamp(at)} · ${shown} across ${spans + 1} stretches`
			: `${stamp(at)} · ${shown}`;
	});

	function whole() {
		pxPerSec = fit;
		if (viewport) viewport.scrollLeft = 0;
		measure();
	}

	function measure() {
		if (!viewport) return;
		width = viewport.clientWidth;
		scroll = viewport.scrollLeft;
	}

	$effect(() => {
		if (!viewport) return;
		const ro = new ResizeObserver(measure);
		ro.observe(viewport);
		measure();
		return () => ro.disconnect();
	});

	/** Framing a selection is how a deep link and the breadcrumb both arrive. */
	$effect(() => {
		const want = frame;
		if (!want || want.kind === 'spawn' || !viewport) return;
		frame = undefined;
		const [span, a, b] = bounds(want);
		const secs = Math.max(1, b - a);
		pxPerSec = Math.min(MAX_PX_PER_SEC, Math.max(fit, (width * 0.8) / secs));
		queueMicrotask(() => {
			if (!viewport) return;
			const t = track;
			const x = place(t, { span, secs: a, inBreak: false });
			const w = place(t, { span, secs: b, inBreak: false }) - x;
			viewport.scrollLeft = x + w / 2 - viewport.clientWidth / 2;
			measure();
		});
	});

	function bounds(sel: Selection): [number, number, number] {
		if (sel.kind === 'window') return [sel.span, sel.from, sel.to];
		// A spawn is not on this timeline at all — phases cut the parent's — so
		// there is nothing to frame and the view stays where it is.
		const p = sel.kind === 'phase' ? facts.phases[sel.phase] : undefined;
		if (!p) return [0, 0, facts.activity.spans[0]?.secs ?? 1];
		const start = Date.parse(facts.activity.spans[p.span]?.started ?? '') || 0;
		return [
			p.span,
			((Date.parse(p.started) || 0) - start) / 1000,
			((Date.parse(p.ended) || 0) - start) / 1000
		];
	}

	/** Zoom about the pointer, so the thing under it stays under it. */
	function zoom(factor: number, anchorX: number) {
		if (!viewport) return;
		const held = locate(track, scroll + anchorX);
		const next = Math.min(MAX_PX_PER_SEC, Math.max(fit, scale * factor));
		if (next === scale) return;
		pxPerSec = next;
		queueMicrotask(() => {
			if (!viewport || !held) return;
			viewport.scrollLeft = place(track, held) - anchorX;
			measure();
		});
	}

	function onwheel(e: WheelEvent) {
		if (e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
		e.preventDefault();
		const box = viewport?.getBoundingClientRect();
		zoom(Math.exp(-e.deltaY * 0.002), box ? e.clientX - box.left : width / 2);
	}

	// Drag to pan, and a click is a drag that went nowhere.
	let dragging = $state(false);
	let moved = 0;
	let anchor = 0;
	let held = 0;

	function down(e: PointerEvent) {
		dragging = true;
		moved = 0;
		anchor = e.clientX;
		held = scroll;
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
	}

	function move(e: PointerEvent) {
		if (!dragging || !viewport) return;
		moved = Math.max(moved, Math.abs(e.clientX - anchor));
		viewport.scrollLeft = held - (e.clientX - anchor);
		measure();
	}

	function up(e: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
		if (moved > 4) return;
		const box = viewport?.getBoundingClientRect();
		if (!box) return;
		pick(scroll + (e.clientX - box.left), e.clientY - box.top);
	}

	/** A click on a band selects the phase; anywhere else, the column under it. */
	function pick(x: number, y: number) {
		const at = locate(track, x);
		if (!at || at.inBreak) return;
		if (y <= BANDS_H) {
			const band = bands.items.find((b) => x >= b.x && x <= b.x + b.w);
			if (band) {
				selection = { kind: 'phase', phase: band.phase };
				return;
			}
		}
		selection = { kind: 'window', ...columnAt(at, res) };
	}

	function onkeydown(e: KeyboardEvent) {
		const step = width / 4;
		if (e.key === '+' || e.key === '=') zoom(1.6, width / 2);
		else if (e.key === '-' || e.key === '_') zoom(1 / 1.6, width / 2);
		else if (e.key === '0') {
			pxPerSec = fit;
			if (viewport) viewport.scrollLeft = 0;
		} else if (e.key === 'ArrowLeft' && viewport) viewport.scrollLeft -= step;
		else if (e.key === 'ArrowRight' && viewport) viewport.scrollLeft += step;
		else return;
		e.preventDefault();
		measure();
	}

	/** The mini-map is the same layout at fit scale — the forest, always whole. */
	const map = $derived.by(() => {
		const px = fitPxPerSec(facts, 1000);
		const r = resolution(px, bucket, false);
		const t = layoutTrack(facts, px, breakWidth(facts, px, 1000, r.secs * px));
		return { t, items: layoutColumns(facts, undefined, t, r)?.items ?? [] };
	});
	const window_ = $derived.by(() => {
		const a = locate(track, scroll);
		const b = locate(track, scroll + width);
		if (!a || !b) return { x: 0, w: map.t.width };
		const x = place(map.t, a);
		return { x, w: Math.max(2, place(map.t, b) - x) };
	});

	/** The stretch of work at the left edge of what is on screen. */
	function currentSpan(): number {
		return locate(track, scroll)?.span ?? 0;
	}

	function spanSecs(): number {
		return Math.max(1, facts.activity.spans[currentSpan()]?.secs ?? 1);
	}

	function jump(e: MouseEvent) {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const at = locate(map.t, ((e.clientX - box.left) / box.width) * map.t.width);
		if (!at || !viewport) return;
		viewport.scrollLeft = place(track, at) - width / 2;
		measure();
	}
</script>

<div class="bar">
	<div class="crumb">
		<button onclick={whole}>whole session</button>
		<button
			onclick={() => (frame = { kind: 'window', span: currentSpan(), from: 0, to: spanSecs() })}
			>this stretch</button
		>
		{#if selection?.kind === 'phase'}
			<button onclick={() => (frame = selection)}>this phase</button>
		{/if}
		<span class="where">{here}</span>
	</div>
	<div class="zoomer">
		<button onclick={() => zoom(1 / 1.6, width / 2)} aria-label="zoom out">−</button>
		<button onclick={() => zoom(1.6, width / 2)} aria-label="zoom in">+</button>
	</div>
</div>

<!--
  `role="application"` is what a pan/zoom surface that takes over the keyboard
  is *for*, and it has to be focusable to receive those keys. Svelte's rule
  classes the role as non-interactive and so flags both the handlers and the
  tabindex; the alternative it would accept — a focusable element per column —
  is the thing this component exists to avoid, at tens of thousands of them.
  The keyboard contract is real and documented in the aria-label: arrows pan,
  +/- zoom, 0 fits.
-->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
<div
	class="viewport {track.density}"
	class:dragging
	bind:this={viewport}
	role="application"
	aria-label="Session timeline — drag to pan, wheel to zoom, click a column for what is behind it"
	tabindex="0"
	onscroll={measure}
	{onwheel}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	{onkeydown}
>
	<div class="track" style="width:{track.width}px;height:{H}px">
		<svg
			style="left:{from}px;width:{to - from}px;height:{H}px"
			viewBox="{from} 0 {to - from} {H}"
			preserveAspectRatio="none"
			aria-hidden="true"
		>
			{#each track.spans as s (s.index)}
				{#if s.index > 0}
					<rect
						class="gap"
						x={s.x - track.breakPx}
						y="0"
						width={track.breakPx}
						height={H - AXIS_H}
					/>
				{/if}
			{/each}
			{#each seen.bands as b (b.phase)}
				<rect
					x={b.x}
					y="0"
					width={Math.max(0.4, b.w)}
					height={BANDS_H}
					fill="var(--ph-{b.kind})"
					opacity={selection?.kind === 'phase' && selection.phase === b.phase ? 1 : 0.82}
				>
					<title>{b.tip}{b.written ? ` — written: ${b.written}` : ''}</title>
				</rect>
			{/each}
			{#each seen.marks as m, i (i)}
				<rect
					x={m.x - 0.5}
					y={BANDS_H + 1}
					width={Math.max(1, Math.min(3, res.secs * scale * 0.6))}
					height={MARKS_H - 3}
					fill={m.kind === 'question' ? 'var(--ask)' : 'var(--user)'}
				>
					<title>{m.tip}</title>
				</rect>
			{/each}
			{#each seen.cols as c, i (i)}
				{#if c.pct > 0}
					<rect
						x={c.x}
						y={H - AXIS_H - (BARS_H * c.pct) / 100}
						width={Math.max(0.6, c.w - Math.min(1, c.w * 0.12))}
						height={(BARS_H * c.pct) / 100}
						fill={c.failed ? 'var(--fail)' : 'var(--bar)'}
					>
						<title>{c.tip}</title>
					</rect>
				{/if}
			{/each}
			{#if selWindow}
				{@const s = track.spans[selWindow.span]}
				{#if s}
					<rect
						class="sel"
						x={s.x + selWindow.from * s.scale}
						y="0"
						width={Math.max(2, (selWindow.to - selWindow.from) * s.scale)}
						height={H - AXIS_H}
					/>
				{/if}
			{:else if selBand}
				<rect class="sel" x={selBand.x} y="0" width={Math.max(2, selBand.w)} height={H - AXIS_H} />
			{/if}
			{#each seen.ticks as t, i (i)}
				<line x1={t.x} y1={H - AXIS_H} x2={t.x} y2={H - AXIS_H + 4} class="tick" />
			{/each}
		</svg>
		{#each seen.ticks as t, i (i)}
			<span class="tl" style="left:{t.x}px">{t.label}</span>
		{/each}
	</div>
</div>

<div class="under">
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="map" onclick={jump} title="The whole session — click to jump">
		<svg viewBox="0 0 {map.t.width} 20" preserveAspectRatio="none" aria-hidden="true">
			{#each map.items as c, i (i)}
				{#if c.pct > 0}
					<rect
						x={c.x}
						y={20 - (20 * c.pct) / 100}
						width={Math.max(0.4, c.w)}
						height={(20 * c.pct) / 100}
						fill="var(--bar)"
					/>
				{/if}
			{/each}
			<rect x={window_.x} y="0" width={window_.w} height="20" class="win" />
		</svg>
	</div>
	<p class="caption">
		{#if cols}
			{#if cols.source === 'facts'}
				<strong>{duration(cols.secs)}</strong> per column, from the facts — the bar counts
				<strong>records</strong>{res.buckets > 1 ? `, ${res.buckets} buckets summed` : ''}.
			{:else}
				<strong>{duration(cols.secs)}</strong> per column, re-bucketed from the events — the bar
				counts <strong>turns and tool calls</strong>. Finer than the
				{duration(bucket)} the facts resolve, so `records` is not available here: it counts system and
				snapshot records the events do not carry.
			{/if}
			{#if cols.unplaced > 0}
				{count(cols.unplaced)} event(s) carried no timestamp and are not drawn.
			{/if}
		{:else}
			Reading the events for this zoom…
		{/if}
		{#if bands.tooNarrow > 0}
			{bands.tooNarrow} phase(s) are too short to draw a band at this zoom — zoom in to see them.
		{/if}
		{#if !zoomed}
			Wheel or <kbd>+</kbd>/<kbd>−</kbd> to zoom, drag or <kbd>←</kbd>/<kbd>→</kbd> to pan.
		{/if}
	</p>
</div>

<p class="legend">
	<span class="key bar"></span>work
	<span class="key barfail"></span>tool failure
	<span class="key mkuser"></span>user prompt
	<span class="key mkask"></span>question asked
	<span class="key gapkey"></span>idle, collapsed
</p>

<style>
	.bar {
		display: flex;
		gap: 12px;
		align-items: center;
		flex-wrap: wrap;
		margin-bottom: 8px;
	}
	.crumb {
		display: flex;
		gap: 6px;
		align-items: center;
		flex-wrap: wrap;
		min-width: 0;
	}
	.zoomer {
		margin-left: auto;
		display: flex;
		gap: 4px;
	}
	button {
		font: inherit;
		font-size: 12px;
		padding: 2px 9px;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--panel);
		color: var(--ink);
		cursor: pointer;
	}
	button:hover {
		background: var(--hover);
	}
	.zoomer button {
		width: 28px;
		padding: 2px 0;
		border-radius: 6px;
		font-size: 14px;
		line-height: 1.1;
	}
	.where {
		font-size: 12px;
		color: var(--muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.viewport {
		position: relative;
		overflow-x: auto;
		overflow-y: hidden;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--bg);
		cursor: grab;
		touch-action: pan-x;
	}
	.viewport:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}
	.viewport.dragging {
		cursor: grabbing;
	}
	.track {
		position: relative;
	}
	svg {
		position: absolute;
		top: 0;
		display: block;
	}
	.gap {
		fill: url(#gaphatch);
	}
	.sel {
		fill: var(--accent);
		opacity: 0.16;
		stroke: var(--accent);
		stroke-width: 1;
		vector-effect: non-scaling-stroke;
	}
	.tick {
		stroke: var(--line);
		stroke-width: 1;
		vector-effect: non-scaling-stroke;
	}
	.tl {
		position: absolute;
		bottom: 0;
		transform: translateX(2px);
		font-size: 10.5px;
		line-height: 16px;
		color: var(--muted);
		white-space: nowrap;
		pointer-events: none;
	}
	.under {
		display: flex;
		gap: 14px;
		align-items: flex-start;
		margin-top: 8px;
	}
	.map {
		flex: 0 0 200px;
		height: 22px;
		border: 1px solid var(--line);
		border-radius: 4px;
		overflow: hidden;
		background: var(--bg);
		cursor: pointer;
	}
	.map svg {
		position: static;
		width: 100%;
		height: 20px;
	}
	.win {
		fill: var(--accent);
		opacity: 0.2;
		stroke: var(--accent);
		stroke-width: 1;
		vector-effect: non-scaling-stroke;
	}
	.caption {
		margin: 0;
		font-size: 11.5px;
		line-height: 1.5;
		color: var(--muted);
	}
	kbd {
		font: inherit;
		font-size: 10.5px;
		border: 1px solid var(--line);
		border-bottom-width: 2px;
		border-radius: 4px;
		padding: 0 4px;
		background: var(--panel);
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
</style>
