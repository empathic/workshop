import {
  getPaneInstanceId,
  getPaneWorkingDir,
  defaultContentForKind,
  migratePaneContentV3toV4,
  PANE_KIND_REGISTRY,
  PANE_KIND_MAP,
  SELECTABLE_KINDS,
  INSTANCE_BOUND_KINDS,
  DIR_BOUND_KINDS,
  PERSISTABLE_CONTENT_KINDS,
  kindLabel,
  kindShortLabel
} from './pane-content.js';
import type { InstanceRef, PaneContentKind } from './pane-content.js';

// =============================================================================
// getPaneInstanceId
// =============================================================================

describe('getPaneInstanceId', () => {
  it('returns instanceId for terminal', () => {
    expect(getPaneInstanceId({ kind: 'terminal', instanceId: 'inst-1' })).toBe('inst-1');
  });

  it('returns null instanceId for terminal', () => {
    expect(getPaneInstanceId({ kind: 'terminal', instanceId: null })).toBeNull();
  });

  it('returns instanceId for conversation', () => {
    expect(getPaneInstanceId({ kind: 'conversation', instanceId: 'inst-2', viewMode: 'structured' })).toBe('inst-2');
  });

  it('returns null for file-explorer', () => {
    expect(getPaneInstanceId({ kind: 'file-explorer', workingDir: '/foo' })).toBeNull();
  });

  it('returns null for tasks', () => {
    expect(getPaneInstanceId({ kind: 'tasks', workingDir: '/bar' })).toBeNull();
  });

  it('returns null for git', () => {
    expect(getPaneInstanceId({ kind: 'git', workingDir: '/baz' })).toBeNull();
  });

  it('returns null for landing', () => {
    expect(getPaneInstanceId({ kind: 'landing' })).toBeNull();
  });

  it('returns null for settings', () => {
    expect(getPaneInstanceId({ kind: 'settings' })).toBeNull();
  });

  it('returns null for chat', () => {
    expect(getPaneInstanceId({ kind: 'chat', scope: 'global' })).toBeNull();
  });

  it('returns null for file-viewer', () => {
    expect(getPaneInstanceId({ kind: 'file-viewer', filePath: '/x.ts', workingDir: null })).toBeNull();
  });

  it('returns null for picker', () => {
    expect(getPaneInstanceId({ kind: 'picker' })).toBeNull();
  });
});

// =============================================================================
// getPaneWorkingDir
// =============================================================================

describe('getPaneWorkingDir', () => {
  const instances: ReadonlyMap<string, InstanceRef> = new Map([
    ['inst-1', { working_dir: '/projects/alpha' }],
    ['inst-2', { working_dir: '/projects/beta' }]
  ]);

  const emptyInstances: ReadonlyMap<string, InstanceRef> = new Map();

  it('returns workingDir directly for file-explorer', () => {
    expect(getPaneWorkingDir({ kind: 'file-explorer', workingDir: '/foo' }, emptyInstances)).toBe('/foo');
  });

  it('returns workingDir directly for tasks', () => {
    expect(getPaneWorkingDir({ kind: 'tasks', workingDir: '/bar' }, emptyInstances)).toBe('/bar');
  });

  it('returns workingDir directly for git', () => {
    expect(getPaneWorkingDir({ kind: 'git', workingDir: '/baz' }, emptyInstances)).toBe('/baz');
  });

  it('returns null workingDir for directory-bound pane with null', () => {
    expect(getPaneWorkingDir({ kind: 'file-explorer', workingDir: null }, emptyInstances)).toBeNull();
  });

  it('resolves working_dir from instance for terminal', () => {
    expect(getPaneWorkingDir({ kind: 'terminal', instanceId: 'inst-1' }, instances)).toBe('/projects/alpha');
  });

  it('resolves working_dir from instance for conversation', () => {
    expect(getPaneWorkingDir({ kind: 'conversation', instanceId: 'inst-2', viewMode: 'raw' }, instances)).toBe(
      '/projects/beta'
    );
  });

  it('returns null for terminal with no matching instance', () => {
    expect(getPaneWorkingDir({ kind: 'terminal', instanceId: 'inst-99' }, instances)).toBeNull();
  });

  it('returns null for terminal with null instanceId', () => {
    expect(getPaneWorkingDir({ kind: 'terminal', instanceId: null }, instances)).toBeNull();
  });

  it('returns workingDir directly for file-viewer', () => {
    expect(getPaneWorkingDir({ kind: 'file-viewer', filePath: '/x.ts', workingDir: '/proj' }, emptyInstances)).toBe(
      '/proj'
    );
  });

  it('returns null for file-viewer with null workingDir', () => {
    expect(getPaneWorkingDir({ kind: 'file-viewer', filePath: '/x.ts', workingDir: null }, emptyInstances)).toBeNull();
  });

  it('returns null for landing', () => {
    expect(getPaneWorkingDir({ kind: 'landing' }, instances)).toBeNull();
  });

  it('returns null for settings', () => {
    expect(getPaneWorkingDir({ kind: 'settings' }, instances)).toBeNull();
  });

  it('returns null for chat', () => {
    expect(getPaneWorkingDir({ kind: 'chat', scope: 'global' }, instances)).toBeNull();
  });

  it('returns null for picker without source context', () => {
    expect(getPaneWorkingDir({ kind: 'picker' }, instances)).toBeNull();
  });

  it('returns sourceWorkingDir for picker with source context', () => {
    expect(getPaneWorkingDir({ kind: 'picker', sourceWorkingDir: '/projects/alpha' }, instances)).toBe(
      '/projects/alpha'
    );
  });

  it('returns null for picker with null sourceWorkingDir', () => {
    expect(getPaneWorkingDir({ kind: 'picker', sourceWorkingDir: null }, instances)).toBeNull();
  });

  it('handles instance without working_dir property', () => {
    const sparse = new Map([['inst-x', {} as InstanceRef]]);
    expect(getPaneWorkingDir({ kind: 'terminal', instanceId: 'inst-x' }, sparse)).toBeNull();
  });
});

// =============================================================================
// defaultContentForKind
// =============================================================================

describe('defaultContentForKind', () => {
  it('creates landing content', () => {
    expect(defaultContentForKind('landing')).toEqual({ kind: 'landing' });
  });

  it('creates terminal with null instanceId (always starts unbound)', () => {
    expect(defaultContentForKind('terminal')).toEqual({ kind: 'terminal', instanceId: null });
  });

  it('creates conversation with null instanceId and structured viewMode', () => {
    expect(defaultContentForKind('conversation')).toEqual({
      kind: 'conversation',
      instanceId: null,
      viewMode: 'structured'
    });
  });

  it('creates file-explorer with workingDir', () => {
    expect(defaultContentForKind('file-explorer', '/projects/alpha')).toEqual({
      kind: 'file-explorer',
      workingDir: '/projects/alpha'
    });
  });

  it('creates file-explorer with null workingDir when not provided', () => {
    expect(defaultContentForKind('file-explorer')).toEqual({ kind: 'file-explorer', workingDir: null });
  });

  it('creates tasks with workingDir', () => {
    expect(defaultContentForKind('tasks', '/proj')).toEqual({ kind: 'tasks', workingDir: '/proj' });
  });

  it('creates git with workingDir', () => {
    expect(defaultContentForKind('git', '/proj')).toEqual({ kind: 'git', workingDir: '/proj' });
  });

  it('instance-bound kinds never carry workingDir', () => {
    const result = defaultContentForKind('terminal', '/foo');
    expect(result).toEqual({ kind: 'terminal', instanceId: null });
    expect('workingDir' in result).toBe(false);
  });

  it('directory-bound kinds never carry instanceId', () => {
    const result = defaultContentForKind('file-explorer', '/foo');
    expect(result).toEqual({ kind: 'file-explorer', workingDir: '/foo' });
    expect('instanceId' in result).toBe(false);
  });

  it('creates chat with global scope (always starts global)', () => {
    expect(defaultContentForKind('chat')).toEqual({ kind: 'chat', scope: 'global' });
  });

  it('creates file-viewer with null filePath and no workingDir', () => {
    expect(defaultContentForKind('file-viewer')).toEqual({
      kind: 'file-viewer',
      filePath: null,
      workingDir: null
    });
  });

  it('creates file-viewer with workingDir', () => {
    expect(defaultContentForKind('file-viewer', '/proj')).toEqual({
      kind: 'file-viewer',
      filePath: null,
      workingDir: '/proj'
    });
  });

  it('creates settings', () => {
    expect(defaultContentForKind('settings')).toEqual({ kind: 'settings' });
  });

  it('creates picker', () => {
    expect(defaultContentForKind('picker')).toEqual({ kind: 'picker' });
  });
});

// =============================================================================
// migratePaneContentV3toV4
// =============================================================================

describe('migratePaneContentV3toV4', () => {
  it('migrates file-explorer from instanceId to workingDir', () => {
    const legacy = { kind: 'file-explorer', instanceId: 'inst-1' };
    expect(migratePaneContentV3toV4(legacy)).toEqual({ kind: 'file-explorer', workingDir: null });
  });

  it('migrates tasks from instanceId to workingDir', () => {
    const legacy = { kind: 'tasks', instanceId: 'inst-2' };
    expect(migratePaneContentV3toV4(legacy)).toEqual({ kind: 'tasks', workingDir: null });
  });

  it('migrates git from instanceId to workingDir', () => {
    const legacy = { kind: 'git', instanceId: null };
    expect(migratePaneContentV3toV4(legacy)).toEqual({ kind: 'git', workingDir: null });
  });

  it('returns null for already-migrated file-explorer', () => {
    const current = { kind: 'file-explorer', workingDir: '/foo' };
    expect(migratePaneContentV3toV4(current)).toBeNull();
  });

  it('returns null for terminal (not a directory-bound kind)', () => {
    const terminal = { kind: 'terminal', instanceId: 'inst-1' };
    expect(migratePaneContentV3toV4(terminal)).toBeNull();
  });

  it('returns null for conversation', () => {
    const convo = { kind: 'conversation', instanceId: 'inst-1', viewMode: 'structured' };
    expect(migratePaneContentV3toV4(convo)).toBeNull();
  });

  it('returns null for landing', () => {
    expect(migratePaneContentV3toV4({ kind: 'landing' })).toBeNull();
  });

  it('returns null for settings', () => {
    expect(migratePaneContentV3toV4({ kind: 'settings' })).toBeNull();
  });

  it('migrates file-viewer without workingDir', () => {
    const legacy = { kind: 'file-viewer', filePath: '/x.ts', lineNumber: 10 };
    expect(migratePaneContentV3toV4(legacy)).toEqual({
      kind: 'file-viewer',
      filePath: '/x.ts',
      lineNumber: 10,
      workingDir: null
    });
  });

  it('returns null for already-migrated file-viewer', () => {
    const current = { kind: 'file-viewer', filePath: '/x.ts', workingDir: '/proj' };
    expect(migratePaneContentV3toV4(current)).toBeNull();
  });
});

// =============================================================================
// PANE_KIND_REGISTRY
// =============================================================================

/** All PaneContentKind values that exist in the type union */
const ALL_KINDS: PaneContentKind[] = [
  'landing',
  'terminal',
  'conversation',
  'file-explorer',
  'chat',
  'tasks',
  'file-viewer',
  'git',
  'settings',
  'picker'
];

describe('PANE_KIND_REGISTRY', () => {
  it('has exactly one entry per PaneContentKind', () => {
    const registryKinds = PANE_KIND_REGISTRY.map((d) => d.kind);
    expect(new Set(registryKinds).size).toBe(registryKinds.length); // no duplicates
    expect(new Set(registryKinds)).toEqual(new Set(ALL_KINDS)); // covers all kinds
  });

  it('PANE_KIND_MAP contains all kinds', () => {
    for (const kind of ALL_KINDS) {
      expect(PANE_KIND_MAP.has(kind)).toBe(true);
    }
  });

  it('every entry has a non-empty label and shortLabel', () => {
    for (const def of PANE_KIND_REGISTRY) {
      expect(def.label.length).toBeGreaterThan(0);
      expect(def.shortLabel.length).toBeGreaterThan(0);
    }
  });

  it('selectable entries have non-empty desc, chromeIcon, and pickerIcon', () => {
    for (const def of SELECTABLE_KINDS) {
      expect(def.desc.length).toBeGreaterThan(0);
      expect(def.chromeIcon.length).toBeGreaterThan(0);
      expect(def.pickerIcon.length).toBeGreaterThan(0);
    }
  });

  it('non-selectable entries are only landing and picker', () => {
    const nonSelectable = PANE_KIND_REGISTRY.filter((d) => !d.selectable).map((d) => d.kind);
    expect(new Set(nonSelectable)).toEqual(new Set(['landing', 'picker']));
  });
});

describe('SELECTABLE_KINDS', () => {
  it('contains exactly the selectable kinds in registry order', () => {
    const expected = PANE_KIND_REGISTRY.filter((d) => d.selectable);
    expect(SELECTABLE_KINDS).toEqual(expected);
  });

  it('does not include landing or picker', () => {
    const kinds = SELECTABLE_KINDS.map((d) => d.kind);
    expect(kinds).not.toContain('landing');
    expect(kinds).not.toContain('picker');
  });

  it('includes all 8 user-facing kinds', () => {
    expect(SELECTABLE_KINDS).toHaveLength(8);
  });
});

describe('INSTANCE_BOUND_KINDS', () => {
  it('contains exactly terminal and conversation', () => {
    expect(INSTANCE_BOUND_KINDS).toEqual(new Set(['terminal', 'conversation']));
  });
});

describe('DIR_BOUND_KINDS', () => {
  it('contains exactly file-explorer, tasks, git, and file-viewer', () => {
    expect(DIR_BOUND_KINDS).toEqual(new Set(['file-explorer', 'tasks', 'git', 'file-viewer']));
  });
});

describe('PERSISTABLE_CONTENT_KINDS', () => {
  it('contains all kinds except picker', () => {
    expect(PERSISTABLE_CONTENT_KINDS).toEqual(
      new Set([
        'landing',
        'terminal',
        'conversation',
        'file-explorer',
        'chat',
        'tasks',
        'file-viewer',
        'git',
        'settings'
      ])
    );
  });

  it('does not contain picker', () => {
    expect(PERSISTABLE_CONTENT_KINDS.has('picker')).toBe(false);
  });
});

describe('kindLabel / kindShortLabel', () => {
  it('returns label for all kinds', () => {
    for (const kind of ALL_KINDS) {
      expect(kindLabel(kind).length).toBeGreaterThan(0);
    }
  });

  it('returns shortLabel for all kinds', () => {
    for (const kind of ALL_KINDS) {
      expect(kindShortLabel(kind).length).toBeGreaterThan(0);
    }
  });

  it('shortLabel is shorter or equal to label', () => {
    for (const kind of ALL_KINDS) {
      expect(kindShortLabel(kind).length).toBeLessThanOrEqual(kindLabel(kind).length);
    }
  });
});

// =============================================================================
// Registry ↔ defaultContentForKind consistency
// =============================================================================

describe('registry ↔ defaultContentForKind consistency', () => {
  it('instance-bound kinds produce content with instanceId, no workingDir', () => {
    for (const kind of INSTANCE_BOUND_KINDS) {
      const content = defaultContentForKind(kind, '/should-be-ignored');
      expect('instanceId' in content).toBe(true);
      expect('workingDir' in content).toBe(false);
    }
  });

  it('directory-bound kinds produce content with workingDir, no instanceId', () => {
    for (const kind of DIR_BOUND_KINDS) {
      const content = defaultContentForKind(kind, '/proj');
      expect('workingDir' in content).toBe(true);
      expect('instanceId' in content).toBe(false);
    }
  });

  it('directory-bound kinds forward the workingDir argument', () => {
    for (const kind of DIR_BOUND_KINDS) {
      const content = defaultContentForKind(kind, '/my-project');
      expect((content as { workingDir: string | null }).workingDir).toBe('/my-project');
    }
  });

  it('none-bound kinds produce content without instanceId or workingDir', () => {
    const noneKinds = PANE_KIND_REGISTRY.filter((d) => d.binding === 'none' && d.selectable).map((d) => d.kind);
    for (const kind of noneKinds) {
      const content = defaultContentForKind(kind);
      expect('instanceId' in content).toBe(false);
      expect('workingDir' in content).toBe(false);
    }
  });
});
