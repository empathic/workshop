/**
 * Project ID Utilities
 *
 * Pure utility for project identification. Extracted from projects.ts to
 * break import cycles between layout.ts and projects.ts (both need the
 * hash function, but projects.ts imports from layout.ts for activeProjectId).
 */

/** Stable hash from a directory path to a project ID string. */
export function projectHash(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = (hash << 5) - hash + char;
    hash |= 0;
  }
  return 'proj-' + Math.abs(hash).toString(36);
}

/** localStorage key for a project's persisted layout. */
export function projectStorageKey(projectId: string): string {
  return `workshop_layout:${projectId}`;
}

/** localStorage key for layout metadata (which project was last active). */
export const LAYOUT_META_KEY = 'workshop_layout:meta';
