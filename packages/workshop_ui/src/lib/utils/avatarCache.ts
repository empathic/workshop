/**
 * Avatar Path Cache
 *
 * Memoizes expensive contour path generation for TopoAvatar.
 * Same identity + type + variant = same SVG paths, no recomputation.
 */

import { contours } from 'd3-contour';
import { geoPath } from 'd3-geo';
import { generateNoiseField, generateAngularNoiseField, hashString } from './noise';
import { updateAvatarMetrics, recordAvatarCacheHit, recordAvatarCacheMiss } from '$lib/stores/metrics';

// =============================================================================
// Types
// =============================================================================

export interface AvatarConfig {
  identity: string;
  type: 'human' | 'agent';
  variant: 'user' | 'assistant' | 'thinking';
  size: number;
}

export interface CachedAvatar {
  paths: string[];
  clipId: string;
  timestamp: number;
}

// =============================================================================
// Constants
// =============================================================================

// Grid size balances detail vs. performance (28x28 = 784 cells, fast enough for real-time)
const GRID_SIZE = 28;

// Separate thresholds for human vs agent avatars because:
// - Humans get smoother contours (more thresholds = more organic curves)
// - Agents get angular contours (fewer thresholds = sharper boundaries)
// This visual distinction helps users quickly identify message source
const HUMAN_THRESHOLDS = [0.1, 0.18, 0.26, 0.34, 0.42, 0.5, 0.58, 0.66, 0.74, 0.82, 0.9];
const AGENT_THRESHOLDS = [0.2, 0.4, 0.6, 0.8];

/**
 * Cache size of 100 because:
 * 1. Each avatar is ~2KB of SVG paths
 * 2. 100 * 2KB = 200KB max memory (acceptable)
 * 3. Typical session sees <50 unique avatars
 * 4. 5-minute TTL means cache naturally clears during long sessions
 */
const MAX_CACHE_SIZE = 100;
const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes

// =============================================================================
// Cache
// =============================================================================

const cache = new Map<string, CachedAvatar>();

function hashConfig(config: AvatarConfig): string {
  return `${config.identity}-${config.type}-${config.variant}-${config.size}`;
}

// =============================================================================
// Path Generation
// =============================================================================

function generateContourPaths(config: AvatarConfig): string[] {
  const isHuman = config.type === 'human';

  // Generate noise field
  const values = isHuman
    ? generateNoiseField(config.identity, GRID_SIZE, GRID_SIZE, 0.04, 1)
    : generateAngularNoiseField(config.identity, GRID_SIZE, GRID_SIZE, 5);

  // Generate contours
  const thresholds = isHuman ? HUMAN_THRESHOLDS : AGENT_THRESHOLDS;
  const contourGenerator = contours().size([GRID_SIZE, GRID_SIZE]).thresholds(thresholds);

  const contourData = contourGenerator(values);

  // Scale path to fit viewBox (32x32)
  const scale = 32 / GRID_SIZE;
  const path = geoPath().projection({
    stream: (s) => ({
      point: (x: number, y: number) => s.point(x * scale, y * scale),
      lineStart: () => s.lineStart(),
      lineEnd: () => s.lineEnd(),
      polygonStart: () => s.polygonStart(),
      polygonEnd: () => s.polygonEnd(),
      sphere: () => {}
    })
  });

  return contourData.map((c) => path(c) || '');
}

// =============================================================================
// Public API
// =============================================================================

/**
 * Get cached avatar paths, generating if needed.
 * Handles cache eviction and TTL automatically.
 * Returns empty avatar on generation failure to prevent crashes.
 */
export function getAvatarPaths(config: AvatarConfig): CachedAvatar {
  const key = hashConfig(config);
  const cached = cache.get(key);

  // Return cached if fresh
  if (cached && Date.now() - cached.timestamp < CACHE_TTL_MS) {
    queueMicrotask(recordAvatarCacheHit);
    return cached;
  }
  queueMicrotask(recordAvatarCacheMiss);

  // Generate new paths with defensive error handling
  let paths: string[];
  try {
    const t0 = performance.now();
    paths = generateContourPaths(config);
    const ms = performance.now() - t0;
    if (ms > 20) {
      console.warn(`[Avatar] generateContourPaths took ${ms.toFixed(1)}ms for ${config.identity}`);
    }
  } catch (e) {
    console.error('[Avatar] Generation failed:', e, config);
    // Return empty but valid avatar instead of crashing
    return { paths: [], clipId: 'error', timestamp: Date.now() };
  }

  const clipId = `topo-clip-${hashString(config.identity)}`;

  const avatar: CachedAvatar = {
    paths,
    clipId,
    timestamp: Date.now()
  };

  // Evict oldest if at capacity
  if (cache.size >= MAX_CACHE_SIZE) {
    let oldestKey: string | null = null;
    let oldestTime = Infinity;

    for (const [k, v] of cache.entries()) {
      if (v.timestamp < oldestTime) {
        oldestTime = v.timestamp;
        oldestKey = k;
      }
    }

    if (oldestKey) {
      cache.delete(oldestKey);
    }
  }

  cache.set(key, avatar);
  queueMicrotask(() => updateAvatarMetrics({ cacheSize: cache.size }));
  return avatar;
}

/**
 * Clear the entire avatar cache.
 * Useful for testing or memory pressure situations.
 */
export function clearAvatarCache(): void {
  cache.clear();
}

/**
 * Get current cache statistics.
 */
export function getAvatarCacheStats(): { size: number; maxSize: number } {
  return {
    size: cache.size,
    maxSize: MAX_CACHE_SIZE
  };
}
