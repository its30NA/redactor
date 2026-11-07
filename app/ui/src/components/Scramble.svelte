<!--
  Text that "decrypts" into place: the real text is in the DOM from the start (and is what a
  screen reader gets); an overlay of shuffling glyphs stands in for it for half a second.
-->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { appearance } from '../lib/appearance.svelte';
  import { decrypt } from '../lib/scramble';

  let { text, play = true, delay = 0 }: { text: string; play?: boolean; delay?: number } = $props();

  let veil: HTMLSpanElement | undefined = $state();
  let running = $state(false);

  onMount(() => {
    if (!play || appearance.reducedMotion) return;
    running = true;
    let stop = () => {};
    let gone = false;
    void tick().then(() => {
      if (gone || !veil) return;
      stop = decrypt(veil, text, { delay, onDone: () => (running = false) });
    });
    return () => {
      gone = true;
      stop();
    };
  });
</script>

<span class="scramble" class:running
  ><span class="txt">{text}</span>{#if running}<span class="veil" aria-hidden="true" bind:this={veil}></span>{/if}</span
>

<style>
  .scramble {
    position: relative;
    display: inline-block;
    max-width: 100%;
    vertical-align: bottom;
  }
  .txt {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .veil {
    position: absolute;
    inset: 0;
    overflow: hidden;
    white-space: pre;
    pointer-events: none;
  }
  .running .txt {
    opacity: 0;
  }
</style>
