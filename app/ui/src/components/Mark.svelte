<!--
  The Redactor mark: a shield crossed by a redaction bar. The bar runs past both edges of the
  shield and the outline breaks around it, so it reads as "blacked out" even at 16 px.
  Geometry is shared with src/assets/mark.svg (the icon source); colours follow the theme.
-->
<script lang="ts">
  let { size = 22, glow = true }: { size?: number; glow?: boolean } = $props();
  const uid = $props.id();
  /** Two concave arcs meet at the crest; straight sides curve into the point. */
  const SHIELD =
    'M12 2.4Q15.2 5 19.4 5.2q.6.05.6.65v5.6c0 4.8-3.3 8.7-8 10.3-4.7-1.6-8-5.5-8-10.3v-5.6q0-.6.6-.65Q8.8 5 12 2.4Z';
</script>

<svg class="mark" class:glow width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
  <defs>
    <linearGradient id="{uid}-g" x1="3" y1="2" x2="21" y2="22" gradientUnits="userSpaceOnUse">
      <stop class="s1" offset="0" />
      <stop class="s2" offset="0.55" />
      <stop class="s3" offset="1" />
    </linearGradient>
    <mask id="{uid}-m" maskUnits="userSpaceOnUse" x="0" y="0" width="24" height="24">
      <rect width="24" height="24" fill="#fff" />
      <rect x="0" y="9.1" width="24" height="6.3" fill="#000" />
    </mask>
  </defs>
  <g mask="url(#{uid}-m)">
    <path class="body" d={SHIELD} />
    <path d={SHIELD} stroke="url(#{uid}-g)" stroke-width="2" stroke-linejoin="round" />
    <path class="line" d="M9.6 7.7h4.8M10.7 17.3h2.6" stroke-width="1.3" stroke-linecap="round" />
  </g>
  <rect x="1.2" y="10.3" width="21.6" height="3.9" rx="1.1" fill="url(#{uid}-g)" />
</svg>

<style>
  .mark {
    display: block;
    overflow: visible;
  }
  .glow {
    filter: drop-shadow(0 0 5px var(--grad-glow));
  }
  .s1 {
    stop-color: var(--grad-a);
  }
  .s2 {
    stop-color: var(--grad-b);
  }
  .s3 {
    stop-color: var(--grad-c);
  }
  .body {
    fill: light-dark(#0b1020, rgb(255 255 255 / 0.06));
  }
  .line {
    stroke: light-dark(rgb(255 255 255 / 0.75), rgb(230 234 242 / 0.6));
  }
</style>
