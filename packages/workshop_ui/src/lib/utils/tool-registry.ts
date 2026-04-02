/**
 * Tool Widget Registry — pure configuration for how each tool renders.
 *
 * No framework imports — Svelte component bindings live in ToolBadges.svelte.
 * Tools with renderMode 'card' need a matching widget in CARD_WIDGETS there.
 */

/** Minimal tool shape (structural subset of ToolCell from types.ts). */
export interface ToolInput {
  name: string;
  input: Record<string, unknown>;
}

/** Badge label for display in the tool badge button. */
export interface BadgeLabel {
  /** Display text (may be truncated). */
  text: string;
  /** 'file' = .tool-file class + accent border; 'detail' = .tool-detail class. */
  style: 'file' | 'detail';
  /** Full text for the tooltip. */
  title: string;
}

/** Labeled field in the expanded tool detail panel. */
export interface ExpandedField {
  label: string;
  value: string;
  clickable?: 'file' | 'glob';
}

export interface ToolWidgetConfig {
  icon: string;
  badgeLabel?: (input: Record<string, unknown>) => BadgeLabel | null;
  expandedFields?: (tool: ToolInput) => ExpandedField[];
  renderMode: 'badge' | 'card';
}

// ── Truncation ──────────────────────────────────────────────────────────────

/** Truncate for badge display (default 40 chars). */
export function truncate(s: string, max = 40): string {
  return s.length > max ? s.slice(0, max) + '\u2026' : s;
}

/** Truncate for expanded field display (default 500 chars). */
export function truncateField(s: string, max = 500): string {
  return s.length > max ? s.slice(0, max) + '\u2026' : s;
}

// ── Badge label helpers ─────────────────────────────────────────────────────

/** Badge label for file-operation tools: shows filename with accent border. */
function fileOpBadgeLabel(input: Record<string, unknown>): BadgeLabel | null {
  const fp = input['file_path'];
  if (typeof fp !== 'string') return null;
  return { text: fp.split('/').pop() ?? fp, style: 'file', title: fp };
}

/** Factory: badge label that extracts a single string key from the input. */
function badgeFromKey(
  key: string,
  style: 'file' | 'detail' = 'detail'
): (input: Record<string, unknown>) => BadgeLabel | null {
  return (input) => {
    const raw = input[key];
    if (typeof raw !== 'string') return null;
    return { text: truncate(raw), style, title: raw };
  };
}

// ── Expanded field helpers ──────────────────────────────────────────────────

/** Expanded fields for file-operation tools (Read, Write, Edit). */
function fileOpExpandedFields(tool: ToolInput): ExpandedField[] {
  const fields: ExpandedField[] = [];
  const input = tool.input;
  if (input['file_path'] != null) fields.push({ label: 'FILE', value: String(input['file_path']), clickable: 'file' });
  if (input['old_string'] != null) fields.push({ label: 'OLD', value: String(input['old_string']) });
  if (input['new_string'] != null) fields.push({ label: 'NEW', value: String(input['new_string']) });
  if (input['content'] != null) fields.push({ label: 'CONTENT', value: truncateField(String(input['content'])) });
  return fields;
}

// ── Registry ────────────────────────────────────────────────────────────────

export const TOOL_REGISTRY: Record<string, ToolWidgetConfig> = {
  Read: {
    icon: '\u{1F4D6}',
    renderMode: 'badge',
    badgeLabel: fileOpBadgeLabel,
    expandedFields: fileOpExpandedFields
  },
  Write: {
    icon: '\u270F\uFE0F',
    renderMode: 'badge',
    badgeLabel: fileOpBadgeLabel,
    expandedFields: fileOpExpandedFields
  },
  Edit: {
    icon: '\u{1F527}',
    renderMode: 'badge',
    badgeLabel: fileOpBadgeLabel,
    expandedFields: fileOpExpandedFields
  },
  Bash: {
    icon: '\u{1F4BB}',
    renderMode: 'badge',
    badgeLabel: badgeFromKey('command'),
    expandedFields: (tool) => {
      const fields: ExpandedField[] = [];
      if (tool.input['command'] != null) fields.push({ label: 'COMMAND', value: String(tool.input['command']) });
      if (tool.input['description'] != null)
        fields.push({ label: 'DESCRIPTION', value: String(tool.input['description']) });
      return fields;
    }
  },
  Glob: {
    icon: '\u{1F50D}',
    renderMode: 'badge',
    badgeLabel: badgeFromKey('pattern', 'file'),
    expandedFields: (tool) => {
      const fields: ExpandedField[] = [];
      if (tool.input['pattern'] != null)
        fields.push({ label: 'PATTERN', value: String(tool.input['pattern']), clickable: 'glob' });
      if (tool.input['path'] != null)
        fields.push({ label: 'PATH', value: String(tool.input['path']), clickable: 'file' });
      return fields;
    }
  },
  Grep: {
    icon: '\u{1F50E}',
    renderMode: 'badge',
    badgeLabel: badgeFromKey('pattern'),
    expandedFields: (tool) => {
      const fields: ExpandedField[] = [];
      if (tool.input['pattern'] != null) fields.push({ label: 'PATTERN', value: String(tool.input['pattern']) });
      if (tool.input['path'] != null)
        fields.push({ label: 'PATH', value: String(tool.input['path']), clickable: 'file' });
      if (tool.input['glob'] != null) fields.push({ label: 'GLOB', value: String(tool.input['glob']) });
      return fields;
    }
  },
  WebFetch: {
    icon: '\u{1F310}',
    renderMode: 'badge',
    badgeLabel: badgeFromKey('url'),
    expandedFields: (tool) => {
      const fields: ExpandedField[] = [];
      if (tool.input['url'] != null) fields.push({ label: 'URL', value: String(tool.input['url']) });
      if (tool.input['prompt'] != null) fields.push({ label: 'PROMPT', value: String(tool.input['prompt']) });
      return fields;
    }
  },
  WebSearch: {
    icon: '\u{1F50D}',
    renderMode: 'badge',
    badgeLabel: badgeFromKey('query'),
    expandedFields: (tool) => {
      const fields: ExpandedField[] = [];
      if (tool.input['query'] != null) fields.push({ label: 'QUERY', value: String(tool.input['query']) });
      return fields;
    }
  },
  Task: {
    icon: '\u{1F4CB}',
    renderMode: 'card'
  },
  AskUserQuestion: {
    icon: '\u2753',
    renderMode: 'card'
  },
  EnterPlanMode: {
    icon: '\u{1F4D0}',
    renderMode: 'badge',
    expandedFields: () => []
  },
  ExitPlanMode: {
    icon: '\u{1F4D0}',
    renderMode: 'card'
  }
};

const DEFAULT_CONFIG: ToolWidgetConfig = {
  icon: '\u26A1',
  renderMode: 'badge',
  expandedFields: (tool) => {
    const fields: ExpandedField[] = [];
    for (const [key, val] of Object.entries(tool.input)) {
      const str = typeof val === 'string' ? val : JSON.stringify(val, null, 2);
      fields.push({ label: key.toUpperCase(), value: truncateField(str) });
    }
    return fields;
  }
};

/** Look up the widget config for a tool, falling back to a generic default. */
export function getToolConfig(name: string): ToolWidgetConfig {
  return TOOL_REGISTRY[name] ?? DEFAULT_CONFIG;
}
