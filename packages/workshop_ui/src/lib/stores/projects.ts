/**
 * Project Grouping Store
 *
 * Groups instances by working_dir into logical "projects".
 * Ordering is persisted to localStorage; everything else is purely derived.
 */

import { derived, writable, get } from 'svelte/store';
import { browser } from '$app/environment';
import { instanceList } from './instances';
import { activeProjectId } from './layout';
import { projectHash } from '$lib/utils/project-id';
import { applyGapReorder, sortByOrder } from '$lib/utils/project-order';
import type { Instance } from '$lib/types';

export interface Project {
  id: string;
  name: string;
  workingDir: string;
  instances: Instance[];
}

/** Extract basename from a path */
function basename(path: string): string {
  const parts = path.replace(/\/+$/, '').split('/');
  return parts[parts.length - 1] || path;
}

// =============================================================================
// Project Order Persistence
// =============================================================================

const PROJECT_ORDER_KEY = 'workshop_project_order';

/** Ordered list of project IDs — drives the sort order of the projects store. */
export const projectOrder = writable<string[]>([]);

/** Load persisted project order from localStorage. Call once on app init. */
export function loadProjectOrder(): void {
  if (!browser) return;
  try {
    const raw = localStorage.getItem(PROJECT_ORDER_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        projectOrder.set(parsed);
      }
    }
  } catch {
    // Corrupt data — start fresh
  }
}

/** Save the current project order to localStorage. */
function saveProjectOrder(): void {
  if (!browser) return;
  try {
    localStorage.setItem(PROJECT_ORDER_KEY, JSON.stringify(get(projectOrder)));
  } catch {
    // Storage full or unavailable
  }
}

/**
 * Move project `fromId` to gap position `gapIndex`.
 *
 * Gap indices represent insertion points between items: for N items there are
 * N+1 gaps (0 = before first item, N = after last). This matches the visual
 * indicator in the Sidebar.
 *
 * Handles the index-space shift caused by removing the source item before
 * reinserting — callers pass the visual gap index directly.
 */
export function reorderProjects(fromId: string, gapIndex: number): void {
  const currentOrder = get(projects).map((p) => p.id);
  const newOrder = applyGapReorder(currentOrder, fromId, gapIndex);
  if (!newOrder) return;

  projectOrder.set(newOrder);
  saveProjectOrder();
}

// =============================================================================
// Derived Stores
// =============================================================================

/** All projects, derived from instanceList grouped by working_dir, sorted by projectOrder */
export const projects = derived([instanceList, projectOrder], ([$list, $order]) => {
  const groups = new Map<string, Instance[]>();
  for (const inst of $list) {
    const dir = inst.working_dir;
    if (!groups.has(dir)) groups.set(dir, []);
    groups.get(dir)!.push(inst);
  }
  const unsorted = Array.from(groups.entries()).map(([dir, insts]) => ({
    id: projectHash(dir),
    name: basename(dir),
    workingDir: dir,
    instances: insts
  }));

  return sortByOrder(unsorted, $order);
});

/** The project containing the currently active layout */
export const currentProject = derived([projects, activeProjectId], ([$projects, $activeId]) => {
  if (!$activeId) return null;
  return $projects.find((p) => p.id === $activeId) ?? null;
});
