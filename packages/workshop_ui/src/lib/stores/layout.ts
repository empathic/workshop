/**
 * Layout Store
 *
 * Binary split pane layout tree. Each node is either a split (two children)
 * or a leaf pane. Panes hold content descriptors that drive component dispatch.
 *
 * All mutations produce new tree references (immutable updates) so Svelte
 * reactivity picks them up without deep comparison.
 */

import { writable, derived, get, readonly } from 'svelte/store';
import type { Readable } from 'svelte/store';
import { browser } from '$app/environment';
import {
  currentInstanceId,
  instances,
  onInstanceDelete,
  onInstanceListReceived,
  driveCurrentInstanceId,
  registerFocusInstance,
  registerProjectSwitch
} from './instances';
import { addToast } from './toasts';
import { updateUrl } from '$lib/utils/url';
import { loadProjectOrder } from './projects';
import {
  getPaneInstanceId,
  getPaneWorkingDir,
  defaultContentForKind,
  migratePaneContentV3toV4,
  PERSISTABLE_CONTENT_KINDS
} from '$lib/utils/pane-content';
import { projectHash, projectStorageKey, LAYOUT_META_KEY } from '$lib/utils/project-id';

// Re-export types and pure helpers so existing imports from 'stores/layout' still work
export type { PaneContent, PaneContentKind } from '$lib/utils/pane-content';
export { getPaneInstanceId, getPaneWorkingDir, defaultContentForKind } from '$lib/utils/pane-content';

// =============================================================================
// Types
// =============================================================================

// Import PaneContent/PaneContentKind for internal use
import type { PaneContent, PaneContentKind } from '$lib/utils/pane-content';

export interface SplitNode {
  type: 'split';
  id: string;
  /** horizontal = top/bottom, vertical = left/right */
  direction: 'horizontal' | 'vertical';
  /** Fraction [0..1] allocated to the first child */
  ratio: number;
  children: [LayoutNode, LayoutNode];
}

export interface LeafNode {
  type: 'pane';
  id: string;
}

export type LayoutNode = SplitNode | LeafNode;

export interface PaneState {
  id: string;
  content: PaneContent;
}

export interface LayoutState {
  root: LayoutNode;
  panes: Map<string, PaneState>;
  focusedPaneId: string;
}

// =============================================================================
// ID Generation
// =============================================================================

let nextId = 1;

function genPaneId(): string {
  return `pane-${nextId++}`;
}

function genSplitId(): string {
  return `split-${nextId++}`;
}

// =============================================================================
// Initial State
// =============================================================================

function createInitialState(): LayoutState {
  const paneId = genPaneId();
  const instanceId = get(currentInstanceId);

  const content: PaneContent = instanceId
    ? { kind: 'conversation', instanceId, viewMode: 'structured' }
    : { kind: 'landing' };

  return {
    root: { type: 'pane', id: paneId },
    panes: new Map([[paneId, { id: paneId, content }]]),
    focusedPaneId: paneId
  };
}

// =============================================================================
// Store
// =============================================================================

export const layoutState = writable<LayoutState>(createInitialState());

// =============================================================================
// Per-Project State
// =============================================================================

/** Internal writable — which project's layout is currently loaded. null = no project (landing). */
const _activeProjectId = writable<string | null>(null);

/** Public readable — current project ID for derived stores. */
export const activeProjectId: Readable<string | null> = readonly(_activeProjectId);

/** Flag: legacy layout loaded from old single-key storage, needs migration on first project switch. */
let _legacyMigrationPending = false;

// =============================================================================
// Derived Stores
// =============================================================================

/** The root node of the layout tree */
export const layoutRoot = derived(layoutState, ($s) => $s.root);

/** The focused pane's state */
export const focusedPane = derived(layoutState, ($s) => {
  return $s.panes.get($s.focusedPaneId) ?? null;
});

/** The focused pane's instance ID (for sidebar highlight etc.) */
export const focusedPaneInstanceId = derived(focusedPane, ($pane) => {
  if (!$pane) return null;
  return getPaneInstanceId($pane.content);
});

/** The focused pane's working directory (resolved from workingDir or instance) */
export const focusedPaneWorkingDir = derived([focusedPane, instances], ([$pane, $instances]) => {
  if (!$pane) return null;
  return getPaneWorkingDir($pane.content, $instances);
});

/** Total number of panes */
export const paneCount = derived(layoutState, ($s) => $s.panes.size);

/** Maximum number of terminal panes allowed */
export const MAX_TERMINAL_PANES = 6;

/** Whether a split handle is currently being dragged */
export const isResizing = writable<boolean>(false);

/** Current count of terminal panes */
export const terminalPaneCount = derived(layoutState, ($s) => {
  let count = 0;
  for (const pane of $s.panes.values()) {
    if (pane.content.kind === 'terminal') count++;
  }
  return count;
});

/** The focused pane's viewMode (null if not a conversation pane) */
export const focusedPaneViewMode = derived(focusedPane, ($pane) => {
  if (!$pane || $pane.content.kind !== 'conversation') return null;
  return $pane.content.viewMode;
});

// =============================================================================
// Per-pane terminal focus requests
// =============================================================================

const _pendingTerminalFocus = new Set<string>();

/** Request that a pane's terminal grab focus on next ready tick */
export function requestTerminalFocus(paneId: string): void {
  _pendingTerminalFocus.add(paneId);
}

/** Consume (read and clear) a pending terminal focus request for a pane */
export function consumeTerminalFocus(paneId: string): boolean {
  if (_pendingTerminalFocus.has(paneId)) {
    _pendingTerminalFocus.delete(paneId);
    return true;
  }
  return false;
}

// =============================================================================
// Instance Focus — the canonical way to change which instance is "current"
// =============================================================================

export type FocusResult = 'focused' | 'swapped' | 'no-pane';

/**
 * Route an instance ID through the layout system.
 *
 * Returns a discriminant so callers can decide what to do next:
 *  - 'focused'  — a pane already showed this instance; we focused it
 *  - 'swapped'  — no pane showed it; we replaced the focused pane's content
 *  - 'no-pane'  — multi-pane and no pane has an instanceId slot (shouldn't happen)
 */
export function setFocusedInstance(id: string | null): FocusResult {
  if (id === null) {
    const focusedId = get(layoutState).focusedPaneId;
    setPaneContent(focusedId, { kind: 'landing' });
    return 'swapped';
  }

  const state = get(layoutState);
  const inst = get(instances).get(id);
  const isStructured = inst ? inst.kind.type === 'Structured' : true;

  // Try to find a pane already showing this instance → focus it
  for (const [paneId, pane] of state.panes) {
    if (getPaneInstanceId(pane.content) === id) {
      // Correct kind mismatch (e.g. conversation pane restored for an unstructured instance)
      if (pane.content.kind === 'conversation' && !isStructured) {
        setPaneContent(paneId, { kind: 'terminal', instanceId: id });
      } else if (pane.content.kind === 'terminal' && isStructured) {
        setPaneContent(paneId, { kind: 'conversation', instanceId: id, viewMode: 'structured' });
      }
      focusPane(paneId);
      return 'focused';
    }
  }

  // No pane shows this instance — bind focused pane to it
  const focusedId = state.focusedPaneId;
  const fp = state.panes.get(focusedId);
  if (fp) {
    if (fp.content.kind === 'terminal' || fp.content.kind === 'conversation') {
      // Instance-bound: swap instanceId, correcting kind mismatch if needed
      if (fp.content.kind === 'conversation' && !isStructured) {
        setPaneContent(focusedId, { kind: 'terminal', instanceId: id });
      } else if (fp.content.kind === 'terminal' && isStructured) {
        setPaneContent(focusedId, { kind: 'conversation', instanceId: id, viewMode: 'structured' });
      } else {
        setPaneContent(focusedId, { ...fp.content, instanceId: id });
      }
    } else {
      // Everything else (landing, picker, file-explorer, etc.): replace with correct kind
      if (isStructured) {
        setPaneContent(focusedId, { kind: 'conversation', instanceId: id, viewMode: 'structured' });
      } else {
        setPaneContent(focusedId, { kind: 'terminal', instanceId: id });
      }
    }
    return 'swapped';
  }

  return 'no-pane';
}

// =============================================================================
// Sync: layout → currentInstanceId (one-way data flow)
// =============================================================================

let _syncSetup = false;

export function setupLayoutSync(): void {
  if (_syncSetup) return;
  _syncSetup = true;

  // Derive an effective instance ID that resolves directory-bound panes to an instance
  const effectiveInstanceId = derived(
    [focusedPaneInstanceId, focusedPaneWorkingDir, instances],
    ([$instId, $wd, $instances]) => {
      if ($instId) return $instId;
      if (!$wd) return null;
      // Find the first instance in this working directory
      for (const inst of $instances.values()) {
        if (inst.working_dir === $wd) return inst.id;
      }
      return null;
    }
  );

  // Drive currentInstanceId from effectiveInstanceId (one-way flow)
  driveCurrentInstanceId(effectiveInstanceId);

  // Register so selectInstance/clearSelection can reach setFocusedInstance
  registerFocusInstance(setFocusedInstance);

  // Register so selectInstance can trigger cross-project switches
  registerProjectSwitch(switchProject);
}

// =============================================================================
// Project Switching
// =============================================================================

/**
 * Switch the active project. Saves the current layout to the current project's
 * key, loads the target project's layout (or creates a default). No-op if
 * already on the target project.
 *
 * Does NOT call setFocusedInstance — the restored layout has its own focus state.
 * Callers that need to focus a specific instance after a cross-project switch
 * (e.g. selectInstance) should call setFocusedInstance themselves.
 */
export function switchProject(targetWorkingDir: string, targetInstanceId?: string | null): void {
  const targetProjectId = projectHash(targetWorkingDir);
  const currentProjId = get(_activeProjectId);

  // No-op if already on this project
  if (targetProjectId === currentProjId) return;

  if (_legacyMigrationPending) {
    // Legacy layout was loaded — save it to the TARGET project key (it belongs there)
    saveLayoutToKey(projectStorageKey(targetProjectId));
    _legacyMigrationPending = false;
    if (browser) {
      try {
        localStorage.removeItem(LEGACY_STORAGE_KEY);
      } catch {
        // ignore
      }
    }
  } else if (currentProjId !== null) {
    // Flush current layout to current project key
    saveLayoutToKey(projectStorageKey(currentProjId));
  }

  // Cancel any pending debounced persist from the old project — it captured the
  // old key, but the layout subscription will fire synchronously below with the
  // new state. Setting activeProjectId *before* layoutState ensures the
  // subscription's persistLayout() reads the correct (new) project key.
  if (_persistTimer) {
    clearTimeout(_persistTimer);
    _persistTimer = null;
  }

  _activeProjectId.set(targetProjectId);
  saveMeta(targetProjectId);

  // Load target project's layout
  const loaded = loadLayoutFromKey(projectStorageKey(targetProjectId));
  if (loaded) {
    // Restored layout has its own pane arrangement and focus — don't mutate it.
    // The caller's targetInstanceId is just a hint (e.g. sidebar picks instances[0]),
    // not necessarily the instance the user had focused when they left this project.
    layoutState.set(loaded);
  } else {
    // No saved layout — create single-pane default for the target instance
    const inst = targetInstanceId ? get(instances).get(targetInstanceId) : null;
    const isStructured = inst ? inst.kind.type === 'Structured' : true;
    const paneId = genPaneId();
    const content: PaneContent = targetInstanceId
      ? isStructured
        ? { kind: 'conversation', instanceId: targetInstanceId, viewMode: 'structured' }
        : { kind: 'terminal', instanceId: targetInstanceId }
      : { kind: 'landing' };
    layoutState.set({
      root: { type: 'pane', id: paneId },
      panes: new Map([[paneId, { id: paneId, content }]]),
      focusedPaneId: paneId
    });
  }
}

// =============================================================================
// Actions
// =============================================================================

/**
 * Split a pane into two. The original pane keeps its content; the new pane
 * gets `newContent` or inherits from the source.
 */
export function splitPane(paneId: string, direction: 'horizontal' | 'vertical', newContent?: PaneContent): void {
  // Capture instances once outside the update (the only external store needed)
  const instMap = newContent ? undefined : get(instances);

  layoutState.update((s) => {
    const sourcePane = s.panes.get(paneId);
    if (!sourcePane) return s;

    const clonedContent: PaneContent = newContent ?? {
      kind: 'picker' as const,
      sourceWorkingDir: getPaneWorkingDir(sourcePane.content, instMap!) ?? undefined
    };

    // Check terminal pane cap
    if (clonedContent.kind === 'terminal') {
      let termCount = 0;
      for (const p of s.panes.values()) {
        if (p.content.kind === 'terminal') termCount++;
      }
      if (termCount >= MAX_TERMINAL_PANES) {
        addToast('Terminal limit reached (max 6)', 'warn');
        return s;
      }
    }

    const newPaneId = genPaneId();
    const splitId = genSplitId();

    const newPane: PaneState = {
      id: newPaneId,
      content: clonedContent
    };

    const splitNode: SplitNode = {
      type: 'split',
      id: splitId,
      direction,
      ratio: 0.5,
      children: [
        { type: 'pane', id: paneId },
        { type: 'pane', id: newPaneId }
      ]
    };

    const newRoot = replaceNode(s.root, paneId, splitNode);
    const newPanes = new Map(s.panes);
    newPanes.set(newPaneId, newPane);

    return {
      root: newRoot,
      panes: newPanes,
      focusedPaneId: newPaneId
    };
  });
}

/**
 * Split the currently focused pane, placing `kind` content in the new pane.
 * Only workingDir context is carried forward — instance binding starts fresh.
 */
export function splitFocusedPane(kind: PaneContentKind): void {
  const s = get(layoutState);
  const focusedId = s.focusedPaneId;
  const focusedContent = s.panes.get(focusedId)?.content ?? { kind: 'terminal', instanceId: null };
  const instMap = get(instances);
  const workingDir =
    getPaneWorkingDir(focusedContent, instMap) ?? instMap.get(get(currentInstanceId) ?? '')?.working_dir ?? null;
  splitPane(focusedId, 'vertical', defaultContentForKind(kind, workingDir));
}

/**
 * Close a pane. Its sibling replaces the parent split node.
 * No-op if this is the last pane.
 */
export function closePane(paneId: string): void {
  layoutState.update((s) => {
    if (s.panes.size <= 1) return s; // Can't close the last pane

    const parentInfo = findParentSplit(s.root, paneId);
    if (!parentInfo) return s;

    const { parent, siblingNode } = parentInfo;

    // Replace the parent split with the sibling
    const newRoot = replaceNode(s.root, parent.id, siblingNode);

    const newPanes = new Map(s.panes);
    newPanes.delete(paneId);

    // If the closed pane was focused, focus the sibling (or first leaf)
    let newFocus = s.focusedPaneId;
    if (newFocus === paneId) {
      newFocus = firstLeafId(siblingNode) ?? Array.from(newPanes.keys())[0];
    }

    return {
      root: newRoot,
      panes: newPanes,
      focusedPaneId: newFocus
    };
  });
}

/** Focus a specific pane */
export function focusPane(paneId: string): void {
  layoutState.update((s) => {
    if (!s.panes.has(paneId)) return s;
    if (s.focusedPaneId === paneId) return s;
    return { ...s, focusedPaneId: paneId };
  });
}

/** Set the split ratio for a split node */
export function setSplitRatio(splitId: string, ratio: number): void {
  const clamped = Math.max(0.15, Math.min(0.85, ratio));
  layoutState.update((s) => {
    const newRoot = updateSplitRatio(s.root, splitId, clamped);
    if (newRoot === s.root) return s;
    return { ...s, root: newRoot };
  });
}

/** Set the content of a pane */
export function setPaneContent(paneId: string, content: PaneContent): void {
  layoutState.update((s) => {
    const pane = s.panes.get(paneId);
    if (!pane) return s;
    // Check terminal pane cap when switching to terminal
    if (content.kind === 'terminal' && pane.content.kind !== 'terminal') {
      let termCount = 0;
      for (const p of s.panes.values()) {
        if (p.content.kind === 'terminal') termCount++;
      }
      if (termCount >= MAX_TERMINAL_PANES) {
        addToast('Terminal limit reached (max 6)', 'warn');
        return s;
      }
    }
    const newPanes = new Map(s.panes);
    newPanes.set(paneId, { ...pane, content });
    return { ...s, panes: newPanes };
  });
}

/**
 * Set the viewMode of a conversation pane. No-op if the pane is not a conversation.
 * Updates the URL `terminal` param for the focused pane.
 */
export function setPaneViewMode(paneId: string, viewMode: 'structured' | 'raw'): void {
  const state = get(layoutState);
  const pane = state.panes.get(paneId);
  if (!pane || pane.content.kind !== 'conversation') return;
  if (pane.content.viewMode === viewMode) return;
  setPaneContent(paneId, { ...pane.content, viewMode });
  if (viewMode === 'raw') {
    requestTerminalFocus(paneId);
  }
  // Sync URL param if this is the focused pane
  if (paneId === state.focusedPaneId) {
    updateUrl({ terminal: viewMode === 'raw' ? 'true' : null });
  }
}

/**
 * Toggle the viewMode of a conversation pane between structured ↔ raw.
 * Returns the new viewMode, or null if the pane is not a conversation.
 */
export function togglePaneViewMode(paneId: string): 'structured' | 'raw' | null {
  const state = get(layoutState);
  const pane = state.panes.get(paneId);
  if (!pane || pane.content.kind !== 'conversation') return null;
  const newMode = pane.content.viewMode === 'structured' ? 'raw' : 'structured';
  setPaneViewMode(paneId, newMode);
  return newMode;
}

/**
 * Move focus to an adjacent pane based on direction.
 * Uses the tree structure to find siblings.
 */
export function moveFocus(direction: 'left' | 'right' | 'up' | 'down'): void {
  layoutState.update((s) => {
    if (s.panes.size <= 1) return s;

    const allLeaves = collectLeaves(s.root);
    const currentIdx = allLeaves.indexOf(s.focusedPaneId);
    if (currentIdx === -1) return s;

    // Simple linear navigation: left/up = previous, right/down = next
    let nextIdx: number;
    if (direction === 'left' || direction === 'up') {
      nextIdx = currentIdx > 0 ? currentIdx - 1 : allLeaves.length - 1;
    } else {
      nextIdx = currentIdx < allLeaves.length - 1 ? currentIdx + 1 : 0;
    }

    const nextPaneId = allLeaves[nextIdx];
    if (nextPaneId === s.focusedPaneId) return s;

    return { ...s, focusedPaneId: nextPaneId };
  });
}

/**
 * Reassign panes that reference a deleted instance.
 * Falls back to the current global instance (which may be null).
 */
export function pruneInstancePanes(deletedInstanceId: string): void {
  layoutState.update((s) => {
    let changed = false;
    const newPanes = new Map(s.panes);
    const fallbackId = get(currentInstanceId);
    for (const [id, pane] of newPanes) {
      const paneInstanceId = getPaneInstanceId(pane.content);
      if (paneInstanceId === deletedInstanceId) {
        if (pane.content.kind === 'terminal' || pane.content.kind === 'conversation') {
          newPanes.set(id, {
            ...pane,
            content: { ...pane.content, instanceId: fallbackId }
          });
        } else if (pane.content.kind === 'chat') {
          newPanes.set(id, {
            ...pane,
            content: { kind: 'chat', scope: 'global' }
          });
        }
        changed = true;
      }
    }
    if (changed) {
      addToast('Pane reassigned \u2014 instance removed', 'info');
      return { ...s, panes: newPanes };
    }
    return s;
  });
}

onInstanceDelete(pruneInstancePanes);

/**
 * Validate all pane instanceId references against the authoritative instance list.
 * Stale references (from a previous server session) are nulled out so PaneHost
 * shows the instance picker instead of a broken view.
 */
export function validatePaneInstances(validIds: Set<string>): void {
  layoutState.update((s) => {
    let changed = false;
    const newPanes = new Map(s.panes);
    for (const [id, pane] of newPanes) {
      const paneInstanceId = getPaneInstanceId(pane.content);
      if (paneInstanceId && !validIds.has(paneInstanceId)) {
        if (pane.content.kind === 'terminal' || pane.content.kind === 'conversation') {
          newPanes.set(id, {
            ...pane,
            content: { ...pane.content, instanceId: null }
          });
        } else if (pane.content.kind === 'chat' && pane.content.scope !== 'global') {
          newPanes.set(id, {
            ...pane,
            content: { kind: 'chat', scope: 'global' }
          });
        }
        changed = true;
      }
    }
    if (!changed) return s;
    return { ...s, panes: newPanes };
  });
}

onInstanceListReceived(validatePaneInstances);

// =============================================================================
// Tree Helpers
// =============================================================================

/** Collect all leaf pane IDs in tree order (left-to-right, top-to-bottom) */
function collectLeaves(node: LayoutNode): string[] {
  if (node.type === 'pane') return [node.id];
  return [...collectLeaves(node.children[0]), ...collectLeaves(node.children[1])];
}

/** Replace a node in the tree by ID, returning a new tree */
function replaceNode(node: LayoutNode, targetId: string, replacement: LayoutNode): LayoutNode {
  if (node.type === 'pane') {
    return node.id === targetId ? replacement : node;
  }

  if (node.id === targetId) return replacement;

  const left = replaceNode(node.children[0], targetId, replacement);
  const right = replaceNode(node.children[1], targetId, replacement);

  if (left === node.children[0] && right === node.children[1]) return node;

  return { ...node, children: [left, right] };
}

/** Find the parent split of a pane, returning the parent and the sibling */
function findParentSplit(
  node: LayoutNode,
  targetId: string,
  parent?: SplitNode,
  childIndex?: 0 | 1
): { parent: SplitNode; siblingNode: LayoutNode } | null {
  if (node.type === 'pane') {
    if (node.id === targetId && parent && childIndex !== undefined) {
      const siblingIndex = childIndex === 0 ? 1 : 0;
      return { parent, siblingNode: parent.children[siblingIndex] };
    }
    return null;
  }

  const left = findParentSplit(node.children[0], targetId, node, 0);
  if (left) return left;

  return findParentSplit(node.children[1], targetId, node, 1);
}

/** Get the first leaf node ID in a subtree */
function firstLeafId(node: LayoutNode): string | null {
  if (node.type === 'pane') return node.id;
  return firstLeafId(node.children[0]);
}

/** Update split ratio for a specific split node */
function updateSplitRatio(node: LayoutNode, splitId: string, ratio: number): LayoutNode {
  if (node.type === 'pane') return node;

  if (node.id === splitId) {
    return { ...node, ratio };
  }

  const left = updateSplitRatio(node.children[0], splitId, ratio);
  const right = updateSplitRatio(node.children[1], splitId, ratio);

  if (left === node.children[0] && right === node.children[1]) return node;

  return { ...node, children: [left, right] };
}

// =============================================================================
// Persistence
// =============================================================================

const LEGACY_STORAGE_KEY = 'workshop_layout';
const LAYOUT_SCHEMA_VERSION = 4;

// Valid content kinds for persistence — derived from the registry (excludes transient 'picker')
const VALID_CONTENT_KINDS = PERSISTABLE_CONTENT_KINDS;

/** JSON-safe representation (Map → array of entries) */
interface SerializedLayoutState {
  version?: number;
  root: LayoutNode;
  panes: [string, PaneState][];
  focusedPaneId: string;
}

function serializeState(state: LayoutState): SerializedLayoutState {
  return {
    version: LAYOUT_SCHEMA_VERSION,
    root: state.root,
    panes: Array.from(state.panes.entries()),
    focusedPaneId: state.focusedPaneId
  };
}

function deserializeState(data: SerializedLayoutState): LayoutState | null {
  try {
    if (!data.root || !Array.isArray(data.panes) || !data.focusedPaneId) return null;

    // Reject future schema versions
    if (data.version !== undefined && data.version > LAYOUT_SCHEMA_VERSION) {
      console.warn(`[layout] Schema version ${data.version} > ${LAYOUT_SCHEMA_VERSION}, discarding`);
      return null;
    }

    const panes = new Map<string, PaneState>(data.panes);
    if (panes.size === 0) return null;

    // Validate content kinds and migrate flat shape to discriminated union
    for (const [id, pane] of panes) {
      if (!VALID_CONTENT_KINDS.has(pane.content.kind)) {
        console.warn(`[layout] Invalid content kind "${pane.content.kind}" in pane ${id}, resetting to terminal`);
        panes.set(id, { ...pane, content: { kind: 'terminal', instanceId: null } });
        continue;
      }
      // Migrate legacy flat PaneContent to discriminated union
      const c = pane.content as Record<string, unknown>;
      if (c.kind === 'file-viewer' && !('filePath' in c)) {
        panes.set(id, { ...pane, content: { kind: 'file-viewer', filePath: null, workingDir: null } });
      } else if (c.kind === 'chat' && !('scope' in c)) {
        panes.set(id, { ...pane, content: { kind: 'chat', scope: (c.instanceId as string) ?? 'global' } });
      } else if (
        c.kind !== 'file-viewer' &&
        c.kind !== 'chat' &&
        c.kind !== 'file-explorer' &&
        c.kind !== 'tasks' &&
        c.kind !== 'git' &&
        !('instanceId' in c)
      ) {
        panes.set(id, { ...pane, content: { kind: c.kind as PaneContentKind, instanceId: null } as PaneContent });
      }
      // v2 → v3: add viewMode to conversation panes
      if (c.kind === 'conversation' && !('viewMode' in c)) {
        panes.set(id, {
          ...pane,
          content: {
            kind: 'conversation',
            instanceId: (c.instanceId as string | null) ?? null,
            viewMode: 'structured'
          }
        });
      }
      // v3 → v4: directory-bound kinds: instanceId → workingDir
      const migrated = migratePaneContentV3toV4(c);
      if (migrated) {
        panes.set(id, { ...pane, content: migrated });
      }
    }

    // Validate tree-pane consistency
    const treeLeaves = new Set(collectLeavesFromNode(data.root));
    const paneIds = new Set(panes.keys());

    // Missing pane for a leaf → corrupt
    for (const leafId of treeLeaves) {
      if (!paneIds.has(leafId)) {
        console.warn(`[layout] Tree leaf "${leafId}" has no matching pane, discarding layout`);
        return null;
      }
    }

    // Extra panes not in tree → delete
    for (const paneId of paneIds) {
      if (!treeLeaves.has(paneId)) {
        console.warn(`[layout] Pane "${paneId}" not referenced by tree, removing`);
        panes.delete(paneId);
      }
    }

    // Clamp split ratios
    clampSplitRatios(data.root);

    // Validate focusedPaneId exists
    if (!panes.has(data.focusedPaneId)) {
      data.focusedPaneId = Array.from(panes.keys())[0];
    }

    // Sync nextId to avoid collisions with restored IDs
    syncNextId(data.root, panes);
    return {
      root: data.root,
      panes,
      focusedPaneId: data.focusedPaneId
    };
  } catch {
    return null;
  }
}

/** Collect all leaf IDs from a tree node */
function collectLeavesFromNode(node: LayoutNode): string[] {
  if (node.type === 'pane') return [node.id];
  return [...collectLeavesFromNode(node.children[0]), ...collectLeavesFromNode(node.children[1])];
}

/** Clamp all split ratios in the tree to [0.15, 0.85] */
function clampSplitRatios(node: LayoutNode): void {
  if (node.type === 'pane') return;
  node.ratio = Math.max(0.15, Math.min(0.85, node.ratio));
  clampSplitRatios(node.children[0]);
  clampSplitRatios(node.children[1]);
}

/** Ensure nextId is higher than any existing ID in the restored state */
function syncNextId(root: LayoutNode, panes: Map<string, PaneState>): void {
  function extractNum(id: string): number {
    const match = id.match(/\d+$/);
    return match ? parseInt(match[0], 10) : 0;
  }
  let maxId = 0;
  function walkTree(node: LayoutNode) {
    maxId = Math.max(maxId, extractNum(node.id));
    if (node.type === 'split') {
      walkTree(node.children[0]);
      walkTree(node.children[1]);
    }
  }
  walkTree(root);
  for (const pane of panes.values()) {
    maxId = Math.max(maxId, extractNum(pane.id));
  }
  nextId = maxId + 1;
}

let _persistTimer: ReturnType<typeof setTimeout> | null = null;

/** Get the current project's storage key, or null if no project is active. */
function currentStorageKey(): string | null {
  const projId = get(_activeProjectId);
  return projId ? projectStorageKey(projId) : null;
}

/** Save a layout state synchronously to a specific key. */
function saveLayoutToKey(key: string): void {
  if (!browser) return;
  try {
    const state = get(layoutState);
    localStorage.setItem(key, JSON.stringify(serializeState(state)));
  } catch {
    // Storage full or unavailable
  }
}

/** Load a layout state from a specific key. Returns null if not found or invalid. */
function loadLayoutFromKey(key: string): LayoutState | null {
  if (!browser) return null;
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    const data = JSON.parse(raw) as SerializedLayoutState;
    return deserializeState(data);
  } catch {
    return null;
  }
}

/** Save metadata (active project ID) to localStorage. */
function saveMeta(activeId: string | null): void {
  if (!browser) return;
  try {
    if (activeId) {
      localStorage.setItem(LAYOUT_META_KEY, JSON.stringify({ activeProjectId: activeId }));
    } else {
      localStorage.removeItem(LAYOUT_META_KEY);
    }
  } catch {
    // ignore
  }
}

/** Debounced save to localStorage (project-scoped) */
function persistLayout(state: LayoutState): void {
  if (!browser) return;
  const key = currentStorageKey();
  if (!key) return; // No project active — don't persist
  if (_persistTimer) clearTimeout(_persistTimer);
  _persistTimer = setTimeout(() => {
    try {
      localStorage.setItem(key, JSON.stringify(serializeState(state)));
    } catch {
      // Storage full or unavailable — silently ignore
    }
  }, 300);
}

/** Flush persistence synchronously (for beforeunload) */
function flushPersist(): void {
  if (!browser) return;
  if (_persistTimer) {
    clearTimeout(_persistTimer);
    _persistTimer = null;
  }
  const key = currentStorageKey();
  if (!key) return;
  try {
    const state = get(layoutState);
    localStorage.setItem(key, JSON.stringify(serializeState(state)));
  } catch {
    // Ignore
  }
}

/** Set up auto-persist on layout changes and flush on beforeunload */
let _persistSetup = false;

export function setupLayoutPersistence(): void {
  if (_persistSetup || !browser) return;
  _persistSetup = true;

  loadProjectOrder();

  layoutState.subscribe((state) => {
    persistLayout(state);
  });

  window.addEventListener('beforeunload', flushPersist);
}

/** Try to restore a saved layout. Only runs once (first mount). */
let _layoutRestored = false;

export function tryRestoreLayout(): boolean {
  if (_layoutRestored) return false;
  _layoutRestored = true;

  if (!browser) return false;

  // 1. Check for meta (new per-project persistence)
  try {
    const metaRaw = localStorage.getItem(LAYOUT_META_KEY);
    if (metaRaw) {
      const meta = JSON.parse(metaRaw) as { activeProjectId?: string };
      if (meta.activeProjectId) {
        const loaded = loadLayoutFromKey(projectStorageKey(meta.activeProjectId));
        if (loaded) {
          layoutState.set(loaded);
          _activeProjectId.set(meta.activeProjectId);
          return true;
        }
      }
    }
  } catch {
    // Corrupt meta — fall through to legacy
  }

  // 2. Check for legacy single-key layout
  try {
    const legacyRaw = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (legacyRaw) {
      const data = JSON.parse(legacyRaw) as SerializedLayoutState;
      const restored = deserializeState(data);
      if (restored) {
        layoutState.set(restored);
        _legacyMigrationPending = true;
        return true;
      }
    }
  } catch {
    // Corrupt legacy — fall through
  }

  // 3. No saved layout — stay with initial state
  return false;
}

// =============================================================================
// Presets
// =============================================================================

export type LayoutPreset = 'single' | 'dev-split' | 'side-by-side';

/** Apply a named layout preset */
export function applyPreset(preset: LayoutPreset): void {
  const instanceId = get(currentInstanceId);

  if (preset === 'single') {
    const paneId = genPaneId();
    layoutState.set({
      root: { type: 'pane', id: paneId },
      panes: new Map([
        [paneId, { id: paneId, content: { kind: 'conversation', instanceId, viewMode: 'structured' } as PaneContent }]
      ]),
      focusedPaneId: paneId
    });
    return;
  }

  if (preset === 'dev-split') {
    // Conversation (60%) | Terminal (40%)
    const convId = genPaneId();
    const termId = genPaneId();
    const splitId = genSplitId();
    layoutState.set({
      root: {
        type: 'split',
        id: splitId,
        direction: 'vertical',
        ratio: 0.6,
        children: [
          { type: 'pane', id: convId },
          { type: 'pane', id: termId }
        ]
      },
      panes: new Map([
        [convId, { id: convId, content: { kind: 'conversation', instanceId, viewMode: 'structured' } as PaneContent }],
        [termId, { id: termId, content: { kind: 'terminal', instanceId: null } as PaneContent }]
      ]),
      focusedPaneId: convId
    });
    return;
  }

  if (preset === 'side-by-side') {
    // Conversation A (50%) | Conversation B (50%)
    const leftId = genPaneId();
    const rightId = genPaneId();
    const splitId = genSplitId();
    layoutState.set({
      root: {
        type: 'split',
        id: splitId,
        direction: 'vertical',
        ratio: 0.5,
        children: [
          { type: 'pane', id: leftId },
          { type: 'pane', id: rightId }
        ]
      },
      panes: new Map([
        [leftId, { id: leftId, content: { kind: 'conversation', instanceId, viewMode: 'structured' } as PaneContent }],
        [rightId, { id: rightId, content: { kind: 'conversation', instanceId, viewMode: 'structured' } as PaneContent }]
      ]),
      focusedPaneId: leftId
    });
    return;
  }
}

/** Clear persisted layout and reset to single pane */
export function resetLayout(): void {
  if (browser) {
    const key = currentStorageKey();
    if (key) localStorage.removeItem(key);
  }
  applyPreset('single');
}
