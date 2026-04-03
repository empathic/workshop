<script lang="ts">
  /**
   * ChannelChange — CRT channel-switch static effect.
   * Plays a 150ms noise→lines→fade transition on instance switch.
   * Respects prefers-reduced-motion (just crossfades).
   */

  import { currentInstanceId } from '$lib/stores/instances';
  import { paneCount } from '$lib/stores/layout';

  let active = $state(false);
  let phase = $state<'noise' | 'lines' | 'fade'>('noise');
  let prevInstanceId: string | null = null;

  // Watch for instance switches — only in single-pane mode.
  // In multi-pane, clicking between panes changes currentInstanceId
  // but all panes are visible so there's no "channel switch".
  $effect(() => {
    const id = $currentInstanceId;
    if (prevInstanceId !== null && id !== prevInstanceId && id !== null && $paneCount <= 1) {
      triggerEffect();
    }
    prevInstanceId = id;
  });

  function triggerEffect() {
    // Check reduced motion preference
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    if (reducedMotion) {
      // Simple crossfade for reduced motion
      active = true;
      phase = 'fade';
      setTimeout(() => {
        active = false;
      }, 150);
      return;
    }

    active = true;
    phase = 'noise';
    setTimeout(() => {
      phase = 'lines';
    }, 50);
    setTimeout(() => {
      phase = 'fade';
    }, 100);
    setTimeout(() => {
      active = false;
    }, 200);
  }
</script>

{#if active}
  <div
    class="channel-change"
    class:noise={phase === 'noise'}
    class:lines={phase === 'lines'}
    class:fade={phase === 'fade'}
  ></div>
{/if}

<style>
  .channel-change {
    position: fixed;
    inset: 0;
    z-index: 9997;
    pointer-events: none;
  }

  /* Phase 1: Random noise */
  .channel-change.noise {
    background: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='4' height='4'%3E%3Crect width='2' height='2' fill='%23fdba74' opacity='0.15'/%3E%3Crect x='2' y='2' width='2' height='2' fill='%23fdba74' opacity='0.08'/%3E%3C/svg%3E")
      repeat;
    opacity: 0.8;
    animation: noise-jitter 0.05s steps(3) infinite;
  }

  @keyframes noise-jitter {
    0% {
      background-position: 0 0;
    }
    33% {
      background-position: 2px 1px;
    }
    66% {
      background-position: -1px 2px;
    }
    100% {
      background-position: 1px -1px;
    }
  }

  /* Phase 2: Horizontal lines (losing signal) */
  .channel-change.lines {
    background: repeating-linear-gradient(
      0deg,
      transparent,
      transparent 3px,
      var(--tint-active) 3px,
      var(--tint-active) 4px
    );
    opacity: 0.6;
  }

  /* Phase 3: Fade out */
  .channel-change.fade {
    background: var(--backdrop);
    opacity: 0;
    transition: opacity 0.1s ease;
  }
</style>
