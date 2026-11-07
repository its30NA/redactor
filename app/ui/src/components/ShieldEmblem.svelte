<!--
  The empty-state emblem: the shield mark inside a glass disc, a conic light ring turning
  slowly around it, a dotted orbit, three particles on their own orbits, a breathing halo and
  a scan glint that crosses the shield now and then.
  Every loop moves only transform or opacity of an element that never repaints, so it runs on
  the compositor; loops pause with the window (.ambient) and stop under reduced motion.
-->
<script lang="ts">
  import Mark from './Mark.svelte';

  let { size = 168 }: { size?: number } = $props();
</script>

<div class="emblem" style:--s="{size}px" aria-hidden="true">
  <span class="halo ambient"></span>
  <span class="orbit ambient">
    <svg viewBox="0 0 100 100"><circle cx="50" cy="50" r="49" /></svg>
  </span>
  <span class="ring ambient"></span>
  <span class="carrier c1 ambient"><span class="p"></span></span>
  <span class="carrier c2 ambient"><span class="p"></span></span>
  <span class="carrier c3 ambient"><span class="p"></span></span>
  <span class="disc">
    <span class="core-glow ambient"></span>
    <span class="mark"><Mark size={Math.round(size * 0.3)} glow={false} /></span>
    <span class="glint-clip"><span class="glint ambient"></span></span>
  </span>
</div>

<style>
  .emblem {
    position: relative;
    width: var(--s);
    height: var(--s);
    display: grid;
    place-items: center;
    pointer-events: none;
  }
  .emblem > span {
    position: absolute;
    border-radius: 50%;
  }

  .halo {
    inset: -30%;
    background:
      radial-gradient(closest-side at 40% 42%, var(--cyan-tint), transparent),
      radial-gradient(closest-side at 62% 60%, var(--violet-tint), transparent);
    animation: breathe 7s var(--ease-in-out) infinite alternate;
  }

  .orbit {
    inset: 0;
    animation: turn 90s linear infinite reverse;
  }
  .orbit svg {
    width: 100%;
    height: 100%;
    display: block;
    overflow: visible;
  }
  .orbit circle {
    fill: none;
    stroke: var(--border-strong);
    stroke-width: 0.6;
    stroke-dasharray: 0.6 3.2;
    stroke-linecap: round;
  }

  /* Conic light ring, masked to a hairline band. */
  .ring {
    inset: 11%;
    background: conic-gradient(
      from 0deg,
      transparent 0deg,
      transparent 40deg,
      color-mix(in srgb, var(--cyan) 90%, transparent) 150deg,
      color-mix(in srgb, var(--violet) 90%, transparent) 250deg,
      transparent 330deg
    );
    mask: radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px));
    filter: drop-shadow(0 0 6px var(--grad-glow));
    animation: turn 14s linear infinite;
  }

  .carrier {
    inset: 0;
  }
  .carrier .p {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 4px;
    height: 4px;
    margin: -2px 0 0 -2px;
    border-radius: 50%;
    background: var(--cyan);
    box-shadow: 0 0 8px 1px var(--cyan);
  }
  .c1 {
    animation: turn 22s linear infinite;
  }
  .c1 .p {
    transform: translateX(calc(var(--s) * 0.5));
  }
  .c2 {
    animation: turn 31s linear infinite reverse;
    rotate: 140deg;
  }
  .c2 .p {
    width: 3px;
    height: 3px;
    background: var(--violet-soft);
    box-shadow: 0 0 8px 1px var(--violet);
    transform: translateX(calc(var(--s) * 0.39));
  }
  .c3 {
    animation: turn 44s linear infinite;
    rotate: 250deg;
  }
  .c3 .p {
    width: 2px;
    height: 2px;
    opacity: 0.8;
    transform: translateX(calc(var(--s) * 0.58));
  }

  .disc {
    inset: 23%;
    display: grid;
    place-items: center;
    background:
      radial-gradient(circle at 50% 0%, var(--panel-sheen-top), transparent 70%),
      var(--glass-strong);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      inset 0 1px 0 0 var(--highlight),
      0 12px 40px -12px light-dark(rgb(11 16 32 / 0.25), rgb(0 0 0 / 0.8)),
      0 0 40px -8px var(--grad-glow);
    overflow: hidden;
  }
  .disc > span {
    position: absolute;
  }
  .core-glow {
    inset: 18%;
    border-radius: 50%;
    background: radial-gradient(closest-side, var(--grad-glow), transparent);
    animation: breathe 5.5s var(--ease-in-out) infinite alternate;
  }
  .mark {
    position: relative;
    display: grid;
    place-items: center;
  }
  .glint-clip {
    inset: 0;
    border-radius: 50%;
    overflow: hidden;
  }
  .glint {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 30%;
    background: linear-gradient(
      180deg,
      transparent,
      color-mix(in srgb, var(--cyan) 22%, transparent) 70%,
      color-mix(in srgb, var(--cyan) 70%, transparent) 96%,
      transparent
    );
    opacity: 0;
    animation: glint 7.5s var(--ease-in-out) infinite;
  }

  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes breathe {
    from {
      transform: scale(0.94);
      opacity: 0.65;
    }
    to {
      transform: scale(1.06);
      opacity: 1;
    }
  }
  @keyframes glint {
    0%,
    62% {
      transform: translateY(-110%);
      opacity: 0;
    }
    66% {
      opacity: 1;
    }
    86% {
      opacity: 0.9;
    }
    100% {
      transform: translateY(360%);
      opacity: 0;
    }
  }
</style>
