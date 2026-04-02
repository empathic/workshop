/**
 * Seeded noise generation for deterministic topographic avatars
 *
 * Strict-mode compliant implementation with proper bounds checking.
 */

// Mulberry32 PRNG - fast, good quality, seedable
function mulberry32(seed: number): () => number {
  return () => {
    let t = (seed += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Hash string to number for seeding
export function hashString(str: string): number {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = (hash << 5) - hash + char;
    hash = hash & hash; // Convert to 32-bit integer
  }
  return Math.abs(hash);
}

// Gradient vectors for 2D Perlin noise (12 vectors)
const GRAD2: ReadonlyArray<readonly [number, number]> = [
  [1, 1],
  [-1, 1],
  [1, -1],
  [-1, -1],
  [1, 0],
  [-1, 0],
  [0, 1],
  [0, -1],
  [1, 1],
  [-1, 1],
  [1, -1],
  [-1, -1]
] as const;

function dot2(g: readonly [number, number], x: number, y: number): number {
  return g[0] * x + g[1] * y;
}

function fade(t: number): number {
  return t * t * t * (t * (t * 6 - 15) + 10);
}

function lerp(a: number, b: number, t: number): number {
  return a + t * (b - a);
}

// Simplex-like 2D noise (simplified gradient noise)
export function createNoiseGenerator(seed: string | number): (x: number, y: number) => number {
  const numericSeed = typeof seed === 'string' ? hashString(seed) : seed;
  const random = mulberry32(numericSeed);

  // Generate permutation table (size 512 for wrap-around)
  const perm = new Uint8Array(512);
  const p = new Uint8Array(256);
  for (let i = 0; i < 256; i++) {
    p[i] = i;
  }

  // Fisher-Yates shuffle
  for (let i = 255; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    // Safe swap - indices are guaranteed to be in bounds
    const pi = p[i]!;
    const pj = p[j]!;
    p[i] = pj;
    p[j] = pi;
  }

  // Duplicate permutation table for overflow handling
  for (let i = 0; i < 512; i++) {
    perm[i] = p[i & 255]!;
  }

  // 2D Perlin-style noise
  return function noise2D(x: number, y: number): number {
    const X = Math.floor(x) & 255;
    const Y = Math.floor(y) & 255;

    const xf = x - Math.floor(x);
    const yf = y - Math.floor(y);

    const u = fade(xf);
    const v = fade(yf);

    // Look up permutation values - indices are masked to 0-255 range
    const permY = perm[Y]!;
    const permY1 = perm[Y + 1]!;
    const aa = perm[X + permY]!;
    const ab = perm[X + permY1]!;
    const ba = perm[X + 1 + permY]!;
    const bb = perm[X + 1 + permY1]!;

    // Get gradient vectors (mod 12 keeps index in bounds of GRAD2)
    const g1 = GRAD2[aa % 12]!;
    const g2 = GRAD2[ba % 12]!;
    const g3 = GRAD2[ab % 12]!;
    const g4 = GRAD2[bb % 12]!;

    const n1 = dot2(g1, xf, yf);
    const n2 = dot2(g2, xf - 1, yf);
    const n3 = dot2(g3, xf, yf - 1);
    const n4 = dot2(g4, xf - 1, yf - 1);

    const x1 = lerp(n1, n2, u);
    const x2 = lerp(n3, n4, u);

    return lerp(x1, x2, v);
  };
}

// Generate a 2D noise field for contour generation
export function generateNoiseField(
  seed: string | number,
  width: number,
  height: number,
  scale: number = 0.15,
  octaves: number = 3
): number[] {
  const noise = createNoiseGenerator(seed);
  const values: number[] = new Array(width * height);

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let value = 0;
      let amplitude = 1;
      let frequency = scale;
      let maxValue = 0;

      for (let o = 0; o < octaves; o++) {
        value += noise(x * frequency, y * frequency) * amplitude;
        maxValue += amplitude;
        amplitude *= 0.5;
        frequency *= 2;
      }

      // Normalize to 0-1 range
      values[y * width + x] = (value / maxValue + 1) / 2;
    }
  }

  return values;
}

// Generate angular/crystalline noise field using Worley/cellular noise
// Creates spiky, triangular patterns ideal for agent avatars
export function generateAngularNoiseField(
  seed: string | number,
  width: number,
  height: number,
  numPoints: number = 8
): number[] {
  const numericSeed = typeof seed === 'string' ? hashString(seed) : seed;
  const random = mulberry32(numericSeed);

  // Generate random feature points
  const points: Array<[number, number]> = [];
  for (let i = 0; i < numPoints; i++) {
    points.push([random() * width, random() * height]);
  }

  const values: number[] = new Array(width * height);

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      // Find distances to two closest points
      let d1 = Infinity;
      let d2 = Infinity;

      for (const point of points) {
        // Use Manhattan distance for more angular results
        const d = Math.abs(x - point[0]) + Math.abs(y - point[1]);
        if (d < d1) {
          d2 = d1;
          d1 = d;
        } else if (d < d2) {
          d2 = d;
        }
      }

      // F2 - F1 creates ridge lines between cells
      values[y * width + x] = d2 - d1;
    }
  }

  // Normalize to 0-1
  let min = Infinity;
  let max = -Infinity;
  for (const v of values) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  const range = max - min || 1;

  for (let i = 0; i < values.length; i++) {
    const v = values[i];
    if (v !== undefined) {
      values[i] = (v - min) / range;
    }
  }

  return values;
}
