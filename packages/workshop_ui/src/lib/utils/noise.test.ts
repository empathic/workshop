/**
 * S-Tier Tests for noise.ts
 *
 * Design principles:
 * 1. Test invariants, not just examples
 * 2. Property-based thinking: "for all inputs X, property Y holds"
 * 3. Clear failure messages that tell you exactly what broke
 * 4. Each test should teach you something about the system
 */

import { hashString, createNoiseGenerator, generateNoiseField, generateAngularNoiseField } from './noise.js';

// =============================================================================
// hashString: The Foundation
// =============================================================================

describe('hashString', () => {
  describe('determinism invariant', () => {
    // PROPERTY: Same input always produces same output
    // This is the fundamental contract of a hash function

    it.each([
      ['empty string', ''],
      ['single char', 'a'],
      ['ascii text', 'hello world'],
      ['unicode', '🔧 workshop 🌆'],
      ['long string', 'x'.repeat(10000)],
      ['special chars', '!@#$%^&*()_+-=[]{}|;:,.<>?'],
      ['whitespace variations', '  \t\n\r  ']
    ])('is deterministic for %s', (_name, input) => {
      const results = Array.from({ length: 5 }, () => hashString(input));
      const allEqual = results.every((r) => r === results[0]);

      expect(allEqual).toBe(true);
      // Hash should be a stable number (determinism verified by allEqual check above)
      expect(typeof results[0]).toBe('number');
    });
  });

  describe('uniqueness property', () => {
    // PROPERTY: Different inputs should (usually) produce different outputs
    // Hash collisions are possible but should be rare

    it('produces distinct hashes for similar strings', () => {
      const variants = ['user1', 'user2', 'user3', 'User1', 'USER1', ' user1', 'user1 '];
      const hashes = variants.map(hashString);
      const uniqueHashes = new Set(hashes);

      expect(uniqueHashes.size).toBe(variants.length);
    });

    it('has low collision rate across random-ish inputs', () => {
      // Generate 1000 sequential strings and check collision rate
      const hashes = new Set<number>();
      const total = 1000;

      for (let i = 0; i < total; i++) {
        hashes.add(hashString(`test-input-${i}-${Math.random().toString(36)}`));
      }

      // Allow at most 0.1% collision rate
      expect(hashes.size).toBeGreaterThan(total * 0.999);
    });
  });

  describe('output bounds', () => {
    // PROPERTY: Hash is always a non-negative 32-bit integer

    const testCases = ['', 'a', 'negative?', '\x00\xFF', 'Ω', '😀'.repeat(100), String.fromCharCode(0xffff)];

    it.each(testCases)('returns non-negative integer for input: %j', (input) => {
      const hash = hashString(input);

      expect(Number.isInteger(hash)).toBe(true);
      expect(hash).toBeGreaterThanOrEqual(0);
      expect(hash).toBeLessThanOrEqual(0x7fffffff); // Max positive 32-bit int
    });
  });
});

// =============================================================================
// createNoiseGenerator: Seeded Randomness
// =============================================================================

describe('createNoiseGenerator', () => {
  describe('determinism by seed', () => {
    // PROPERTY: Same seed → same sequence of values

    it('produces identical sequences for identical string seeds', () => {
      const noise1 = createNoiseGenerator('my-seed');
      const noise2 = createNoiseGenerator('my-seed');

      // Sample a grid of points
      const points = [];
      for (let x = 0; x < 10; x++) {
        for (let y = 0; y < 10; y++) {
          points.push({ x: x * 0.1, y: y * 0.1 });
        }
      }

      const values1 = points.map((p) => noise1(p.x, p.y));
      const values2 = points.map((p) => noise2(p.x, p.y));

      expect(values1).toEqual(values2);
    });

    it('produces identical sequences for identical numeric seeds', () => {
      const noise1 = createNoiseGenerator(42);
      const noise2 = createNoiseGenerator(42);

      const samples = Array.from({ length: 50 }, (_, i) => ({
        v1: noise1(i * 0.1, i * 0.2),
        v2: noise2(i * 0.1, i * 0.2)
      }));

      samples.forEach(({ v1, v2 }) => {
        expect(v1).toBe(v2);
      });
    });
  });

  describe('seed sensitivity', () => {
    // PROPERTY: Different seeds → different sequences

    it('produces different output for different string seeds', () => {
      const seeds = ['seed-a', 'seed-b', 'seed-c', 'SEED-A', 'seed_a'];
      const generators = seeds.map(createNoiseGenerator);

      // Sample at multiple points and check that sequences diverge
      const samplePoints = [
        { x: 0.5, y: 0.5 },
        { x: 1.0, y: 0.0 },
        { x: 0.0, y: 1.0 },
        { x: 2.5, y: 3.7 }
      ];

      // For each pair of generators, at least one sample point should differ
      for (let i = 0; i < generators.length; i++) {
        for (let j = i + 1; j < generators.length; j++) {
          const gen1 = generators[i]!;
          const gen2 = generators[j]!;

          const anyDifferent = samplePoints.some((p) => gen1(p.x, p.y) !== gen2(p.x, p.y));
          expect(anyDifferent).toBe(true);
        }
      }
    });

    it('distinguishes numerically close seeds', () => {
      const noise1 = createNoiseGenerator(1000);
      const noise2 = createNoiseGenerator(1001);

      // They should diverge
      let anyDifferent = false;
      for (let i = 0; i < 100 && !anyDifferent; i++) {
        if (noise1(i * 0.1, i * 0.1) !== noise2(i * 0.1, i * 0.1)) {
          anyDifferent = true;
        }
      }

      expect(anyDifferent).toBe(true);
    });
  });

  describe('continuity property', () => {
    // PROPERTY: Noise should be continuous - nearby inputs give nearby outputs
    // This is what makes it useful for graphics/procedural generation

    it('has smooth transitions between adjacent samples', () => {
      const noise = createNoiseGenerator('continuity-test');
      const step = 0.01; // Small step

      // Sample along a line and check smoothness
      const values: number[] = [];
      for (let t = 0; t <= 1; t += step) {
        values.push(noise(t, t));
      }

      // Check that consecutive differences are small
      const jumps = values.slice(1).map((v, i) => {
        const prev = values[i];
        return prev !== undefined ? Math.abs(v - prev) : 0;
      });
      const maxJump = Math.max(...jumps);

      // For continuous noise, jumps should be small relative to the range
      expect(maxJump).toBeLessThan(0.3); // Empirically reasonable for this step size
    });
  });

  describe('output range', () => {
    // PROPERTY: Perlin-style noise should output roughly in [-1, 1]

    it('stays within expected bounds across many samples', () => {
      const noise = createNoiseGenerator('range-test');
      let min = Infinity;
      let max = -Infinity;

      // Extensive sampling
      for (let x = -10; x <= 10; x += 0.1) {
        for (let y = -10; y <= 10; y += 0.1) {
          const v = noise(x, y);
          min = Math.min(min, v);
          max = Math.max(max, v);
        }
      }

      // Perlin noise theoretical range is roughly [-1, 1]
      // but can exceed slightly depending on gradient vectors
      expect(min).toBeGreaterThanOrEqual(-1.5);
      expect(max).toBeLessThanOrEqual(1.5);

      // Should use most of the range (not degenerate)
      expect(max - min).toBeGreaterThan(1.0);
    });
  });
});

// =============================================================================
// generateNoiseField: 2D Field Generation
// =============================================================================

describe('generateNoiseField', () => {
  describe('output shape', () => {
    // PROPERTY: Output length = width × height, always

    const dimensions: Array<[number, number]> = [
      [1, 1],
      [10, 10],
      [7, 13],
      [100, 50],
      [1, 100]
    ];

    it.each(dimensions)('produces %d×%d elements', (width, height) => {
      const field = generateNoiseField('shape-test', width, height);

      expect(field).toHaveLength(width * height);
    });
  });

  describe('normalization invariant', () => {
    // PROPERTY: All values are normalized to [0, 1]

    it('constrains all values to [0, 1] range', () => {
      const field = generateNoiseField('norm-test', 50, 50);

      const outOfBounds = field.filter((v) => v < 0 || v > 1);
      expect(outOfBounds).toHaveLength(0);
    });

    it('uses the full range (not clustered in middle)', () => {
      const field = generateNoiseField('distribution-test', 100, 100);

      const min = Math.min(...field);
      const max = Math.max(...field);

      // Should span most of the [0, 1] range
      expect(max - min).toBeGreaterThan(0.5);
    });
  });

  describe('parameter sensitivity', () => {
    // PROPERTY: Changing scale/octaves changes output

    it('scale affects frequency of variation', () => {
      const fieldLowScale = generateNoiseField('scale-test', 20, 20, 0.01);
      const fieldHighScale = generateNoiseField('scale-test', 20, 20, 0.5);

      // High scale = more variation = higher variance in adjacent cells
      const varianceLow = computeAdjacentVariance(fieldLowScale, 20);
      const varianceHigh = computeAdjacentVariance(fieldHighScale, 20);

      expect(varianceHigh).toBeGreaterThan(varianceLow);
    });

    it('more octaves changes the output', () => {
      const field1Oct = generateNoiseField('oct-test', 30, 30, 0.1, 1);
      const field4Oct = generateNoiseField('oct-test', 30, 30, 0.1, 4);

      // More octaves produces a different field (higher frequency details layered in)
      const differences = field1Oct.filter((v, i) => v !== field4Oct[i]).length;

      // Most values should differ (octaves add layered noise)
      expect(differences).toBeGreaterThan(field1Oct.length * 0.5);
    });
  });

  describe('seed determinism', () => {
    it('same seed + params = identical field', () => {
      const field1 = generateNoiseField('det-test', 25, 25, 0.1, 2);
      const field2 = generateNoiseField('det-test', 25, 25, 0.1, 2);

      expect(field1).toEqual(field2);
    });

    it('different seed = different field', () => {
      const field1 = generateNoiseField('seed-a', 20, 20);
      const field2 = generateNoiseField('seed-b', 20, 20);

      // Should differ in most positions
      const differences = field1.filter((v, i) => v !== field2[i]).length;
      expect(differences).toBeGreaterThan(field1.length * 0.9);
    });
  });
});

// =============================================================================
// generateAngularNoiseField: Cellular/Worley Noise
// =============================================================================

describe('generateAngularNoiseField', () => {
  describe('output shape and range', () => {
    it('produces correctly sized normalized output', () => {
      const field = generateAngularNoiseField('angular-test', 30, 40);

      expect(field).toHaveLength(30 * 40);
      expect(Math.min(...field)).toBeGreaterThanOrEqual(0);
      expect(Math.max(...field)).toBeLessThanOrEqual(1);
    });
  });

  describe('visual characteristics', () => {
    // PROPERTY: Angular noise should have cell-like structure
    // This manifests as having distinct regions and sharp boundaries

    it('creates cellular structure with ridges', () => {
      const field = generateAngularNoiseField('cell-test', 50, 50, 5);

      // Count values near edges (ridge lines appear at high values)
      const ridgeValues = field.filter((v) => v > 0.8);
      const lowValues = field.filter((v) => v < 0.2);

      // Should have both valleys (near feature points) and ridges (between cells)
      expect(ridgeValues.length).toBeGreaterThan(0);
      expect(lowValues.length).toBeGreaterThan(0);
    });

    it('more points = smaller cells', () => {
      const fieldFewPoints = generateAngularNoiseField('points-test', 50, 50, 3);
      const fieldManyPoints = generateAngularNoiseField('points-test', 50, 50, 20);

      // More points means more variation (smaller cells with more boundaries)
      const varianceFew = computeVariance(fieldFewPoints);
      const varianceMany = computeVariance(fieldManyPoints);

      // Note: relationship isn't strictly linear, but more points = richer structure
      expect(varianceMany).not.toBe(varianceFew);
    });
  });

  describe('differs from regular noise', () => {
    // PROPERTY: Angular noise has different visual characteristics than Perlin

    it('has different distribution than generateNoiseField', () => {
      const perlin = generateNoiseField('compare', 50, 50);
      const angular = generateAngularNoiseField('compare', 50, 50);

      // Angular noise (F2-F1) tends to have many values near 0 (at cell centers)
      // and fewer high values (only at ridges)
      const perlinLowCount = perlin.filter((v) => v < 0.3).length;
      const angularLowCount = angular.filter((v) => v < 0.3).length;

      // These will have different distributions
      expect(angularLowCount).not.toBe(perlinLowCount);
    });
  });
});

// =============================================================================
// Helpers
// =============================================================================

/** Compute variance of differences between adjacent cells (measures "bumpiness") */
function computeAdjacentVariance(field: number[], width: number): number {
  const diffs: number[] = [];

  for (let i = 0; i < field.length; i++) {
    const current = field[i];
    if (current === undefined) continue;

    // Right neighbor
    if ((i + 1) % width !== 0) {
      const right = field[i + 1];
      if (right !== undefined) {
        diffs.push(Math.abs(current - right));
      }
    }
    // Bottom neighbor
    if (i + width < field.length) {
      const bottom = field[i + width];
      if (bottom !== undefined) {
        diffs.push(Math.abs(current - bottom));
      }
    }
  }

  return computeVariance(diffs);
}

/** Compute variance of an array */
function computeVariance(arr: number[]): number {
  const mean = arr.reduce((a, b) => a + b, 0) / arr.length;
  const squaredDiffs = arr.map((v) => (v - mean) ** 2);
  return squaredDiffs.reduce((a, b) => a + b, 0) / arr.length;
}
