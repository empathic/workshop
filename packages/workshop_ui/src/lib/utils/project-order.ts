/**
 * Pure helpers for project ordering.
 *
 * Extracted from stores/projects.ts so the logic is testable without
 * Svelte stores or browser APIs.
 */

/**
 * Check whether a gap index would produce a real move for the item at fromIdx.
 * Returns false for gaps immediately before or after the source (no-ops).
 */
export function isGapValid(fromIdx: number, gapIndex: number): boolean {
  if (fromIdx === -1) return false;
  return gapIndex !== fromIdx && gapIndex !== fromIdx + 1;
}

/**
 * Apply a gap-based reorder to an array of IDs.
 *
 * Gap indices represent insertion points between items: for N items there
 * are N+1 gaps (0 = before first, N = after last).
 *
 * Returns a new array with `fromId` moved to the gap, or null if the
 * operation is a no-op (fromId not found, or gap is adjacent to fromId).
 */
export function applyGapReorder(ids: string[], fromId: string, gapIndex: number): string[] | null {
  const fromIdx = ids.indexOf(fromId);
  if (!isGapValid(fromIdx, gapIndex)) return null;

  const result = [...ids];
  result.splice(fromIdx, 1);
  // After removal, gap indices above the source shift down by 1
  const insertIdx = Math.min(gapIndex > fromIdx ? gapIndex - 1 : gapIndex, result.length);
  result.splice(insertIdx, 0, fromId);
  return result;
}

/**
 * Sort items by a persisted order array.
 *
 * Items whose IDs appear in `order` come first (in that order).
 * Items not in `order` are appended in their original relative order.
 */
export function sortByOrder<T extends { id: string }>(items: T[], order: string[]): T[] {
  if (order.length === 0) return items;

  const orderIndex = new Map(order.map((id, i) => [id, i]));
  return [...items].sort((a, b) => {
    const ai = orderIndex.get(a.id);
    const bi = orderIndex.get(b.id);
    if (ai !== undefined && bi !== undefined) return ai - bi;
    if (ai !== undefined) return -1;
    if (bi !== undefined) return 1;
    return 0;
  });
}
