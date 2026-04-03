<script lang="ts">
  /**
   * BaudMeter - Activity indicator LED
   *
   * Single dot that flickers like a hard drive activity light.
   * Brightness and glow intensity vary with output activity.
   */

  interface Props {
    /** Activity level 0-1 */
    level?: number;
    /** Whether this instance is actively working (for simulation) */
    active?: boolean;
    /** Color for the LED */
    color?: string;
    /** Whether this is stale/uncertain state */
    stale?: boolean;
  }

  let { level = 0, active = false, color = 'var(--chrome-accent-500)', stale = false }: Props = $props();

  // Simulated activity for non-focused active instances
  let simulatedLevel = $state(0);
  let animationFrame: number;

  $effect(() => {
    if (active && level === 0) {
      // Simulate flickering activity like a hard drive LED
      const animate = () => {
        // Random flicker between 0.3 and 1.0 for that hard drive light feel
        simulatedLevel = 0.3 + Math.random() * 0.7;
        // Flicker at ~15-30fps for realistic LED look
        setTimeout(
          () => {
            animationFrame = requestAnimationFrame(animate);
          },
          30 + Math.random() * 40
        );
      };
      animate();

      return () => {
        cancelAnimationFrame(animationFrame);
        simulatedLevel = 0;
      };
    } else {
      simulatedLevel = 0;
    }
  });

  // Use real level if provided, otherwise simulated
  let effectiveLevel = $derived(level > 0 ? level : simulatedLevel);

  // Calculate opacity and glow based on activity
  let opacity = $derived(0.4 + effectiveLevel * 0.6);
  let glowSize = $derived(4 + effectiveLevel * 8);
</script>

<span
  class="activity-led"
  class:active={effectiveLevel > 0}
  class:stale
  style="--led-color: {color}; --led-opacity: {opacity}; --glow-size: {glowSize}px"
></span>

<style>
  .activity-led {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--led-color);
    opacity: var(--led-opacity, 0.4);
    box-shadow: 0 0 var(--glow-size, 4px) var(--led-color);
    transition:
      opacity 0.05s,
      box-shadow 0.05s;
  }

  .activity-led.stale {
    opacity: 0.3;
    box-shadow: none;
  }
</style>
