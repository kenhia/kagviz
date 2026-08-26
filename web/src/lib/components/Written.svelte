<!--
  The model-written headline — the only prose on the page nobody counted.

  Styled so it can never be mistaken for one of the numbers: no stat card, no
  large numeral, an accent rule down the side, a serif face nothing else uses,
  and an attribution naming the model in the block itself. Absent when the
  facts carry no labels, which is the default path — so the page below has to
  look finished without it, not broken.
-->
<script lang="ts">
	import type { Labels } from '$lib/contract/facts.js';

	let { labels }: { labels: Labels | undefined } = $props();
</script>

{#if labels}
	<section class="written">
		<p class="said">{labels.headline}</p>
		<p class="by">
			<span class="mark">written</span> by {labels.model} ({labels.prompt_version}) over the facts
			below. It and the marked phase labels are the only text here that was not measured.
		</p>
	</section>
{/if}
