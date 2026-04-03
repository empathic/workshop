/**
 * Pane Content — types, registry, and pure helpers.
 *
 * This module is the single source of truth for pane kind metadata.
 * The PANE_KIND_REGISTRY defines every kind's label, icon, binding type,
 * and display properties. All derived lookups (SELECTABLE_KINDS,
 * INSTANCE_BOUND_KINDS, etc.) flow from the registry.
 *
 * No store dependencies — safe to import from tests and non-Svelte modules.
 */

// =============================================================================
// Types (re-exported by stores/layout.ts)
// =============================================================================

export type PaneContentKind =
  | 'landing'
  | 'terminal'
  | 'conversation'
  | 'file-explorer'
  | 'chat'
  | 'tasks'
  | 'file-viewer'
  | 'git'
  | 'settings'
  | 'picker';

export type PaneContent =
  | { kind: 'landing' }
  | { kind: 'terminal'; instanceId: string | null }
  | { kind: 'conversation'; instanceId: string | null; viewMode: 'structured' | 'raw' }
  | {
      kind: 'file-viewer';
      filePath: string | null;
      lineNumber?: number;
      workingDir: string | null;
      diffContext?: { commit?: string; base?: string; head?: string; diffMode?: string };
    }
  | { kind: 'file-explorer'; workingDir: string | null }
  | { kind: 'chat'; scope: 'global' | string }
  | { kind: 'tasks'; workingDir: string | null }
  | { kind: 'git'; workingDir: string | null }
  | { kind: 'settings' }
  | { kind: 'picker'; sourceWorkingDir?: string | null };

/** Minimal instance shape needed by getPaneWorkingDir */
export interface InstanceRef {
  working_dir?: string;
}

// =============================================================================
// Pane Kind Registry
// =============================================================================

/** How a pane kind resolves its project context */
export type PaneBindingType = 'instance' | 'directory' | 'none';

export interface PaneKindDef {
  kind: PaneContentKind;
  /** Full display label (e.g. "Terminal", "Conversation") */
  label: string;
  /** Short label for mobile tabs and compact display (e.g. "Convo", "Files") */
  shortLabel: string;
  /** One-line description for the kind picker */
  desc: string;
  /** How this kind resolves project context */
  binding: PaneBindingType;
  /** Whether this kind appears in the chrome dropdown and kind picker */
  selectable: boolean;
  /** SVG inner content for 16x16 viewBox (chrome split popover) */
  chromeIcon: string;
  /** SVG inner content for 20x20 viewBox (kind picker cards) */
  pickerIcon: string;
}

/**
 * The single source of truth for all pane kind metadata.
 * Order determines display order in the kind picker and chrome dropdown.
 */
export const PANE_KIND_REGISTRY: readonly PaneKindDef[] = [
  {
    kind: 'terminal',
    label: 'Terminal',
    shortLabel: 'Terminal',
    desc: 'Shell session',
    binding: 'instance',
    selectable: true,
    chromeIcon: '<rect x="2" y="3" width="12" height="10" rx="1"/><polyline points="4 7 6 9 4 11"/>',
    pickerIcon:
      '<rect x="2" y="3" width="16" height="13" rx="1.5"/><polyline points="5 8 7.5 10.5 5 13"/><line x1="10" y1="13" x2="14" y2="13"/>'
  },
  {
    kind: 'conversation',
    label: 'Conversation',
    shortLabel: 'Convo',
    desc: 'Claude transcript',
    binding: 'instance',
    selectable: true,
    chromeIcon: '<path d="M2 4h12a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-3l-2 2v-2H2a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/>',
    pickerIcon: '<path d="M3 4h14a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1h-4l-3 3v-3H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z"/>'
  },
  {
    kind: 'file-explorer',
    label: 'Files',
    shortLabel: 'Files',
    desc: 'Browse project tree',
    binding: 'directory',
    selectable: true,
    chromeIcon:
      '<path d="M1 4V3.5A.5.5 0 0 1 1.5 3H5l1.5 1.5H14.5a.5.5 0 0 1 .5.5v.5"/><rect x="1" y="4.5" width="14" height="9" rx=".5"/>',
    pickerIcon:
      '<path d="M2 5V4a1 1 0 0 1 1-1h5l2 2h7a1 1 0 0 1 1 1v1"/><rect x="2" y="5" width="16" height="11" rx="1"/>'
  },
  {
    kind: 'chat',
    label: 'Chat',
    shortLabel: 'Chat',
    desc: 'Team messages',
    binding: 'none',
    selectable: true,
    chromeIcon:
      '<circle cx="8" cy="8" r="6"/><circle cx="5.5" cy="7.5" r=".7"/><circle cx="8" cy="7.5" r=".7"/><circle cx="10.5" cy="7.5" r=".7"/>',
    pickerIcon:
      '<circle cx="10" cy="10" r="7.5"/><circle cx="6.5" cy="9.5" r="1"/><circle cx="10" cy="9.5" r="1"/><circle cx="13.5" cy="9.5" r="1"/>'
  },
  {
    kind: 'tasks',
    label: 'Tasks',
    shortLabel: 'Tasks',
    desc: 'Todo tracking',
    binding: 'directory',
    selectable: true,
    chromeIcon:
      '<rect x="2" y="2" width="4" height="4" rx=".5"/><rect x="2" y="10" width="4" height="4" rx=".5"/><line x1="9" y1="4" x2="14" y2="4"/><line x1="9" y1="12" x2="14" y2="12"/>',
    pickerIcon:
      '<rect x="3" y="3" width="5" height="5" rx="0.5"/><rect x="3" y="12" width="5" height="5" rx="0.5"/><line x1="11" y1="5.5" x2="17" y2="5.5"/><line x1="11" y1="14.5" x2="17" y2="14.5"/>'
  },
  {
    kind: 'file-viewer',
    label: 'File Viewer',
    shortLabel: 'Viewer',
    desc: 'Read & diff files',
    binding: 'directory',
    selectable: true,
    chromeIcon:
      '<path d="M4 1h6l3 3v10a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5V1.5A.5.5 0 0 1 4 1z"/><polyline points="9.5 1 9.5 4.5 13 4.5"/>',
    pickerIcon:
      '<path d="M5 2h8l4 4v12a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z"/><polyline points="12 2 12 6 17 6"/><line x1="7" y1="10" x2="14" y2="10"/><line x1="7" y1="13" x2="12" y2="13"/>'
  },
  {
    kind: 'git',
    label: 'Git',
    shortLabel: 'Git',
    desc: 'Commits & diffs',
    binding: 'directory',
    selectable: true,
    chromeIcon:
      '<circle cx="8" cy="3" r="1.5"/><circle cx="8" cy="13" r="1.5"/><circle cx="13" cy="8" r="1.5"/><line x1="8" y1="4.5" x2="8" y2="11.5"/><path d="M8 6.5c0 1.5 5 1.5 5 1.5"/>',
    pickerIcon:
      '<circle cx="10" cy="4" r="2"/><circle cx="10" cy="16" r="2"/><circle cx="16" cy="10" r="2"/><line x1="10" y1="6" x2="10" y2="14"/><path d="M10 8c0 2 6 2 6 2"/>'
  },
  {
    kind: 'settings',
    label: 'Settings',
    shortLabel: 'Settings',
    desc: 'Preferences',
    binding: 'none',
    selectable: true,
    chromeIcon:
      '<circle cx="8" cy="8" r="2.5"/><path d="M8 1.5v2m0 9v2M3.4 3.4l1.4 1.4m6.4 6.4l1.4 1.4M1.5 8h2m9 0h2M3.4 12.6l1.4-1.4m6.4-6.4l1.4-1.4"/>',
    pickerIcon:
      '<circle cx="10" cy="10" r="3"/><path d="M10 2v2m0 12v2M4.2 4.2l1.4 1.4m8.8 8.8l1.4 1.4M2 10h2m12 0h2M4.2 15.8l1.4-1.4m8.8-8.8l1.4-1.4"/>'
  },
  {
    kind: 'landing',
    label: 'Landing',
    shortLabel: 'Home',
    desc: '',
    binding: 'none',
    selectable: false,
    chromeIcon: '',
    pickerIcon: ''
  },
  {
    kind: 'picker',
    label: 'Picker',
    shortLabel: 'Picker',
    desc: '',
    binding: 'none',
    selectable: false,
    chromeIcon: '',
    pickerIcon: ''
  }
] as const;

// =============================================================================
// Derived Lookups
// =============================================================================

/** Kind → definition lookup */
export const PANE_KIND_MAP: ReadonlyMap<PaneContentKind, PaneKindDef> = new Map(
  PANE_KIND_REGISTRY.map((def) => [def.kind, def])
);

/** Kinds that appear in the chrome dropdown and kind picker (in display order) */
export const SELECTABLE_KINDS: readonly PaneKindDef[] = PANE_KIND_REGISTRY.filter((d) => d.selectable);

/** Kinds that carry instanceId (terminal, conversation) */
export const INSTANCE_BOUND_KINDS: ReadonlySet<PaneContentKind> = new Set(
  PANE_KIND_REGISTRY.filter((d) => d.binding === 'instance').map((d) => d.kind)
);

/** Kinds that carry workingDir (file-explorer, tasks, git, file-viewer) */
export const DIR_BOUND_KINDS: ReadonlySet<PaneContentKind> = new Set(
  PANE_KIND_REGISTRY.filter((d) => d.binding === 'directory').map((d) => d.kind)
);

/** Kinds that can be persisted to localStorage (everything except transient picker) */
export const PERSISTABLE_CONTENT_KINDS: ReadonlySet<string> = new Set(
  PANE_KIND_REGISTRY.filter((d) => d.kind !== 'picker').map((d) => d.kind)
);

/** Full display label for a kind */
export function kindLabel(kind: PaneContentKind): string {
  return PANE_KIND_MAP.get(kind)?.label ?? kind;
}

/** Short label for mobile tabs and compact displays */
export function kindShortLabel(kind: PaneContentKind): string {
  return PANE_KIND_MAP.get(kind)?.shortLabel ?? kind;
}

// =============================================================================
// Helpers
// =============================================================================

/** Extract instanceId from a PaneContent (terminal/conversation only) */
export function getPaneInstanceId(content: PaneContent): string | null {
  if (content.kind === 'terminal' || content.kind === 'conversation') return content.instanceId;
  return null;
}

/**
 * Extract workingDir from a PaneContent.
 *
 * Directory-bound kinds (file-explorer, tasks, git) return their workingDir directly.
 * Instance-bound kinds (terminal, conversation) resolve via the provided instance map.
 * Picker panes return their sourceWorkingDir.
 * All other kinds return null.
 */
export function getPaneWorkingDir(content: PaneContent, instanceMap: ReadonlyMap<string, InstanceRef>): string | null {
  if ('workingDir' in content) return content.workingDir;
  if (content.kind === 'picker') {
    return content.sourceWorkingDir ?? null;
  }
  const id = getPaneInstanceId(content);
  if (!id) return null;
  return instanceMap.get(id)?.working_dir ?? null;
}

/**
 * Construct default PaneContent for a given kind.
 *
 * Only `workingDir` is carried as cross-kind context. Instance-bound kinds
 * (terminal, conversation) start with `instanceId: null` — the instance picker
 * or auto-creation flow handles binding. This prevents accidentally wiring
 * an incompatible instance (e.g. a shell) into the wrong kind (e.g. conversation).
 */
export function defaultContentForKind(kind: PaneContentKind, workingDir: string | null = null): PaneContent {
  switch (kind) {
    case 'landing':
      return { kind: 'landing' };
    case 'terminal':
      return { kind: 'terminal', instanceId: null };
    case 'file-explorer':
    case 'tasks':
    case 'git':
      return { kind, workingDir };
    case 'conversation':
      return { kind: 'conversation', instanceId: null, viewMode: 'structured' };
    case 'file-viewer':
      return { kind: 'file-viewer', filePath: null, workingDir };
    case 'chat':
      return { kind: 'chat', scope: 'global' };
    case 'settings':
      return { kind: 'settings' };
    case 'picker':
      return { kind: 'picker' };
  }
}

/**
 * Migrate a legacy pane content record (v3 schema) to v4.
 * Converts directory-bound kinds from instanceId to workingDir.
 * Returns null if no migration was needed.
 */
export function migratePaneContentV3toV4(content: Record<string, unknown>): PaneContent | null {
  const kind = content['kind'];
  if (
    (kind === 'file-explorer' || kind === 'tasks' || kind === 'git') &&
    'instanceId' in content &&
    !('workingDir' in content)
  ) {
    return { kind: kind as 'file-explorer' | 'tasks' | 'git', workingDir: null };
  }
  // v4 → v5: file-viewer gains workingDir
  if (kind === 'file-viewer' && !('workingDir' in content)) {
    const lineNumber = content['lineNumber'] as number | undefined;
    return {
      kind: 'file-viewer' as const,
      filePath: (content['filePath'] as string | null) ?? null,
      ...(lineNumber != null ? { lineNumber } : {}),
      workingDir: null
    };
  }
  return null;
}
