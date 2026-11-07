<!--
  The seam between Input and Safe output. At rest a quiet node; while a scan runs, light
  pulses travel across it from left to right (transform and opacity only, and only while busy).
-->
<script lang="ts">
  import ChevronsRight from '@lucide/svelte/icons/chevrons-right';

  let { busy = false, active = false }: { busy?: boolean; active?: boolean } = $props();
</script>

<div class="seam" class:busy class:active aria-hidden="true">
  <span class="line"></span>
  {#if busy}
    <span class="flow">
      <span class="pulse p1"></span>
      <span class="pulse p2"></span>
      <span class="pulse p3"></span>
    </span>
  {/if}
  <span class="node"><ChevronsRight size={14} strokeWidth={2} /></span>
</div>

<style>
  .seam {
    position: relative;
    display: grid;
    place-items: center;
  }
  .line {
    position: absolute;
    top: 28%;
    bottom: 28%;
    left: 50%;
    width: 1px;
    background: linear-gradient(
      180deg,
      transparent,
      color-mix(in srgb, var(--cyan) 45%, transparent) 30%,
      color-mix(in srgb, var(--violet) 45%, transparent) 70%,
      transparent
    );
    opacity: 0.7;
    transition: opacity var(--dur-3) ease;
  }
  .active .line {
    opacity: 1;
  }
  .node {
    position: relative;
    z-index: 1;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    color: var(--text-2);
    background: var(--glass-strong);
    box-shadow:
      inset 0 0 0 1px var(--border-strong),
      inset 0 1px 0 0 var(--highlight),
      0 0 18px -2px var(--grad-glow);
    transition:
      color var(--dur-2) ease,
      box-shadow var(--dur-3) ease;
  }
  .active .node {
    color: var(--mint);
  }
  .busy .node {
    color: var(--cyan);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--cyan) 50%, transparent),
      inset 0 1px 0 0 var(--highlight),
      0 0 22px 0 color-mix(in srgb, var(--cyan) 40%, transparent);
  }

  /* Pulses cross the gap horizontally at the node's height. */
  .flow {
    position: absolute;
    left: -8px;
    right: -8px;
    top: 50%;
    height: 2px;
    margin-top: -1px;
    overflow: hidden;
  }
  .pulse {
    position: absolute;
    left: 0;
    top: 0;
    width: 14px;
    height: 2px;
    border-radius: 1px;
    background: linear-gradient(90deg, transparent, var(--cyan), transparent);
    opacity: 0;
    animation: cross 900ms cubic-bezier(0.4, 0, 0.6, 1) infinite;
    will-change: transform, opacity;
  }
  .p2 {
    animation-delay: 300ms;
  }
  .p3 {
    animation-delay: 600ms;
  }
  @keyframes cross {
    0% {
      transform: translateX(-14px);
      opacity: 0;
    }
    25% {
      opacity: 1;
    }
    75% {
      opacity: 1;
    }
    100% {
      transform: translateX(38px);
      opacity: 0;
    }
  }
</style>
