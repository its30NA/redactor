<!--
  Glass panel: translucent fill over the aurora, 1px border, top inner highlight, soft drop.
  No backdrop-filter on purpose: large blurred panels over an animated backdrop would be
  re-blurred every frame. The aurora is already soft, so translucency alone reads as glass.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';

  let {
    children,
    tone = 'none',
    class: klass = '',
    ...rest
  }: HTMLAttributes<HTMLElement> & {
    children: Snippet;
    /** Edge glow, e.g. cyan while the editor has focus, red while secrets are showing. */
    tone?: 'none' | 'cyan' | 'violet' | 'red' | 'mint';
  } = $props();
</script>

<section class="panel {klass}" data-tone={tone === 'none' ? undefined : tone} {...rest}>
  {@render children()}
</section>

<style>
  .panel {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-lg);
    background:
      linear-gradient(180deg, var(--panel-sheen-top), var(--panel-sheen-bottom) 120px, transparent 100%),
      var(--panel-fill);
    box-shadow: var(--shadow-panel);
    overflow: hidden;
    transition: box-shadow var(--dur-3) ease;
  }
  .panel[data-tone] {
    box-shadow:
      var(--shadow-panel),
      0 0 0 1px color-mix(in srgb, var(--tone) 45%, transparent),
      0 0 36px -10px color-mix(in srgb, var(--tone) 55%, transparent);
  }
  .panel[data-tone='cyan'] {
    --tone: var(--cyan);
  }
  .panel[data-tone='violet'] {
    --tone: var(--violet);
  }
  .panel[data-tone='red'] {
    --tone: var(--red);
  }
  .panel[data-tone='mint'] {
    --tone: var(--mint);
  }
  /* Rim light: a 1px gradient border, bright where light falls on the top edge. */
  .panel::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 2;
    padding: 1px;
    border-radius: inherit;
    background: linear-gradient(180deg, var(--rim-top), var(--border) 22%, var(--rim-bottom) 100%);
    mask:
      linear-gradient(#000 0 0) content-box exclude,
      linear-gradient(#000 0 0);
    pointer-events: none;
  }
</style>
