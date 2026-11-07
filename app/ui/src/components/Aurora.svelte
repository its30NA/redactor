<!--
  The backdrop: two slow aurora blobs (cyan, violet) plus a faint third tint, a dot grid that
  fades out towards the edges, film grain and a vignette.
  GPU-cheap by construction: the blobs are soft radial gradients (no blur filter) rasterised
  once and moved only with transform; everything else is static. The loops pause while the
  window is inactive and are off under reduced motion.
-->
<div class="aurora" aria-hidden="true">
  <div class="blob a ambient"></div>
  <div class="blob b ambient"></div>
  <div class="blob c ambient"></div>
  <div class="dots"></div>
  <div class="grain"></div>
  <div class="vignette"></div>
</div>

<style>
  .aurora {
    position: fixed;
    inset: 0;
    z-index: var(--z-aurora);
    overflow: hidden;
    pointer-events: none;
    background: var(--bg);
    contain: strict;
  }

  .blob {
    position: absolute;
    border-radius: 50%;
    will-change: transform;
    backface-visibility: hidden;
  }
  /* Centres: a at (16vw, 6vh), b at (90vw, 104vh), c at (62vw, 24vh). */
  .a {
    left: -29vw;
    top: calc(6vh - 45vw);
    width: 90vw;
    height: 90vw;
    background: radial-gradient(
      closest-side,
      var(--aurora-a) 0%,
      color-mix(in srgb, var(--aurora-a) 55%, transparent) 38%,
      color-mix(in srgb, var(--aurora-a) 18%, transparent) 68%,
      transparent 100%
    );
    animation: drift-a 46s var(--ease-in-out) infinite alternate;
  }
  .b {
    left: 42vw;
    top: calc(104vh - 48vw);
    width: 96vw;
    height: 96vw;
    background: radial-gradient(
      closest-side,
      var(--aurora-b) 0%,
      color-mix(in srgb, var(--aurora-b) 55%, transparent) 38%,
      color-mix(in srgb, var(--aurora-b) 18%, transparent) 68%,
      transparent 100%
    );
    animation: drift-b 54s var(--ease-in-out) infinite alternate;
  }
  .c {
    left: 36vw;
    top: calc(24vh - 17vw);
    width: 52vw;
    height: 34vw;
    background: radial-gradient(closest-side, var(--aurora-c), transparent);
    animation: drift-c 38s var(--ease-in-out) infinite alternate;
  }

  .dots {
    position: absolute;
    inset: 0;
    background-image: radial-gradient(circle, var(--dots) 0.9px, transparent 1.3px);
    background-size: 22px 22px;
    background-position: 11px 11px;
    mask-image: radial-gradient(ellipse 75% 65% at 50% 42%, #000 15%, transparent 78%);
  }

  .grain {
    position: absolute;
    inset: 0;
    background-color: var(--grain);
    mask-image: url('../assets/grain.svg');
    mask-size: 200px 200px;
  }

  .vignette {
    position: absolute;
    inset: 0;
    background: radial-gradient(ellipse 120% 95% at 50% 38%, transparent 55%, var(--vignette) 100%);
  }
</style>
