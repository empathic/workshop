<script lang="ts">
  import { getAvatarPaths } from '$lib/utils/avatarCache';

  interface Props {
    /** Unique identifier to seed the terrain (username, agent id, etc) */
    identity: string;
    /** Avatar type: 'human' for smooth contours, 'agent' for angular/spiky */
    type?: 'human' | 'agent';
    /** Visual variant affecting color scheme */
    variant?: 'user' | 'assistant' | 'thinking';
    /** Size in pixels */
    size?: number;
    /** Whether to show subtle animation */
    animated?: boolean;
  }

  let { identity, type = 'agent', variant = 'assistant', size = 28, animated = false }: Props = $props();

  // Get cached avatar paths (memoized - only regenerates on config change)
  const avatar = $derived(getAvatarPaths({ identity, type, variant, size }));

  // Read a CSS variable from the active theme
  function cssVar(name: string, fallback: string): string {
    if (typeof document === 'undefined') return fallback;
    return getComputedStyle(document.body).getPropertyValue(name).trim() || fallback;
  }

  // Color schemes for different variants — read from theme variables
  const colors = $derived.by(() => {
    switch (variant) {
      case 'user':
        return {
          bg: cssVar('--surface-900', '#030806'),
          border: cssVar('--status-green-border', '#0d2a20'),
          stroke: cssVar('--status-green-text', '#4ade80')
        };
      case 'thinking':
        return {
          bg: cssVar('--surface-900', '#06030a'),
          border: cssVar('--tint-thinking-strong', '#1a102a'),
          stroke: cssVar('--thinking-400', '#a78bfa')
        };
      case 'assistant':
      default:
        return {
          bg: cssVar('--surface-900', '#050302'),
          border: cssVar('--surface-border', '#2a1a0a'),
          stroke: cssVar('--status-yellow', '#fbbf24')
        };
    }
  });
</script>

<svg
  class="topo-avatar"
  class:animated
  viewBox="0 0 32 32"
  width={size}
  height={size}
  style="--border-color: {colors.border};"
>
  <defs>
    <clipPath id={avatar.clipId}>
      <circle cx="16" cy="16" r="15" />
    </clipPath>
  </defs>

  <!-- Background circle - solid color -->
  <circle cx="16" cy="16" r="15" fill={colors.bg} stroke={colors.border} stroke-width="1" />

  <!-- Contour lines (from cache) -->
  <g clip-path="url(#{avatar.clipId})">
    {#each avatar.paths as path, i}
      <path
        d={path}
        fill="none"
        stroke={colors.stroke}
        stroke-width={0.75}
        stroke-linecap="round"
        class="contour-line"
        style="--delay: {i * 0.06}s"
      />
    {/each}
  </g>

  <!-- Subtle inner glow -->
  <circle cx="16" cy="16" r="14" fill="none" stroke={colors.stroke} stroke-width="0.5" stroke-opacity="0.2" />
</svg>

<style>
  .topo-avatar {
    display: block;
    border-radius: 50%;
  }

  .contour-line {
    vector-effect: non-scaling-stroke;
  }

  /* Subtle pulse animation */
  .topo-avatar.animated .contour-line {
    animation: contour-pulse 3s ease-in-out infinite;
    animation-delay: var(--delay);
  }

  @keyframes contour-pulse {
    0%,
    100% {
      stroke-opacity: 0.4;
    }
    50% {
      stroke-opacity: 0.8;
    }
  }

</style>
