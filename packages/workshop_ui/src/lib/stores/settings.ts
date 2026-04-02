import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';
import { api, apiGet } from '$lib/utils/api';

// =============================================================================
// Types
// =============================================================================

export type ThemeId = 'phosphor' | 'analog' | 'solarized-dark' | 'solarized-light' | 'darcula' | 'intellij-light';

export const THEME_OPTIONS: readonly { id: ThemeId; label: string; family: 'dark' | 'light' }[] = [
  { id: 'phosphor', label: 'Phosphor', family: 'dark' },
  { id: 'analog', label: 'Analog', family: 'light' },
  { id: 'solarized-dark', label: 'Solarized Dark', family: 'dark' },
  { id: 'solarized-light', label: 'Solarized Light', family: 'light' },
  { id: 'darcula', label: 'Darcula', family: 'dark' },
  { id: 'intellij-light', label: 'IntelliJ Light', family: 'light' }
] as const;

export interface UserSettings {
  theme: ThemeId;
  defaultCommand: string;
  shellCommand: string;
  diffEngine: 'standard' | 'patience' | 'structural';
  terminalFontSize: number;
  terminalFontFamily: string;
  showNotifications: boolean;
  drawerWidth: number;
}

export const DEFAULT_SETTINGS: UserSettings = {
  theme: 'phosphor',
  defaultCommand: 'claude',
  shellCommand: 'bash',
  diffEngine: 'structural',
  terminalFontSize: 14,
  terminalFontFamily: "'JetBrains Mono', 'SF Mono', Monaco, 'Cascadia Code', monospace",
  showNotifications: true,
  drawerWidth: 400
};

// Keys that are UI-only and should NOT sync to the server
const LOCAL_ONLY_KEYS: ReadonlySet<string> = new Set(['drawerWidth']);

const STORAGE_KEY = 'workshop_settings';

// =============================================================================
// Migration: old localStorage keys → unified store
// =============================================================================

function migrateOldKeys(): Partial<UserSettings> {
  if (!browser) return {};
  const migrated: Partial<UserSettings> = {};

  const oldTheme = localStorage.getItem('workshop_theme');
  if (oldTheme) {
    migrated.theme = JSON.parse(oldTheme) as ThemeId;
    localStorage.removeItem('workshop_theme');
  }

  const oldCommand = localStorage.getItem('workshop_default_command');
  if (oldCommand) {
    migrated.defaultCommand = JSON.parse(oldCommand);
    localStorage.removeItem('workshop_default_command');
  }

  const oldDrawerWidth = localStorage.getItem('workshop_drawer_width');
  if (oldDrawerWidth) {
    migrated.drawerWidth = JSON.parse(oldDrawerWidth);
    localStorage.removeItem('workshop_drawer_width');
  }

  const oldDiffEngine = localStorage.getItem('workshop_diff_engine');
  if (oldDiffEngine) {
    migrated.diffEngine = JSON.parse(oldDiffEngine) as UserSettings['diffEngine'];
    localStorage.removeItem('workshop_diff_engine');
  }

  return migrated;
}

function loadFromLocalStorage(): UserSettings {
  if (!browser) return { ...DEFAULT_SETTINGS };

  // Try unified store first
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
    }
  } catch {
    // corrupt — fall through
  }

  // Try migrating old keys
  const migrated = migrateOldKeys();
  const settings = { ...DEFAULT_SETTINGS, ...migrated };

  // Save unified
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // storage full
  }

  return settings;
}

function saveToLocalStorage(settings: UserSettings): void {
  if (!browser) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // storage full
  }
}

// =============================================================================
// Store
// =============================================================================

export const userSettings = writable<UserSettings>(loadFromLocalStorage());

// Auto-persist to localStorage on change
if (browser) {
  userSettings.subscribe(saveToLocalStorage);
}

// =============================================================================
// Convenience derived stores (for backward compatibility)
// =============================================================================

// These writable stores proxy to userSettings so existing imports keep working.

function createSettingProxy<K extends keyof UserSettings>(key: K) {
  const store = writable<UserSettings[K]>(get(userSettings)[key]);

  // Sync from userSettings → proxy
  userSettings.subscribe((s) => {
    store.set(s[key]);
  });

  return store;
}

export const theme = createSettingProxy('theme');
export const defaultCommand = createSettingProxy('defaultCommand');
export const diffEngine = createSettingProxy('diffEngine');
export const drawerWidth = createSettingProxy('drawerWidth');

// drawerOpen is ephemeral UI state, not a setting
export const drawerOpen = writable(false);

// =============================================================================
// Actions
// =============================================================================

/** Update a single setting: writes to store, localStorage, and PATCHes server */
export function updateSetting<K extends keyof UserSettings>(key: K, value: UserSettings[K]): void {
  userSettings.update((s) => ({ ...s, [key]: value }));

  // Sync to server (unless local-only)
  if (!LOCAL_ONLY_KEYS.has(key)) {
    patchServer({ [key]: String(value) });
  }
}

/** Batch update settings */
export function updateSettings(partial: Partial<UserSettings>): void {
  userSettings.update((s) => ({ ...s, ...partial }));

  // Sync non-local keys to server
  const serverUpdates: Record<string, string> = {};
  for (const [key, value] of Object.entries(partial)) {
    if (!LOCAL_ONLY_KEYS.has(key)) {
      serverUpdates[key] = String(value);
    }
  }
  if (Object.keys(serverUpdates).length > 0) {
    patchServer(serverUpdates);
  }
}

export function setTheme(id: ThemeId): void {
  updateSetting('theme', id);
}

export function toggleTheme(): void {
  const current = get(userSettings).theme;
  const ids = THEME_OPTIONS.map((t) => t.id);
  const idx = ids.indexOf(current);
  const next = ids[(idx + 1) % ids.length];
  updateSetting('theme', next);
}

export function toggleDrawer(): void {
  drawerOpen.update((open) => !open);
}

export function setDrawerOpen(open: boolean): void {
  drawerOpen.set(open);
}

export function setDrawerWidth(width: number): void {
  const clamped = Math.max(200, Math.min(800, width));
  updateSetting('drawerWidth', clamped);
}

// =============================================================================
// Server Sync
// =============================================================================

/** PATCH settings to server (fire-and-forget) */
function patchServer(updates: Record<string, string>): void {
  api('/api/user/settings', {
    method: 'PATCH',
    body: JSON.stringify(updates)
  }).catch((e) => {
    console.warn('[settings] Failed to sync to server:', e);
  });
}

/** Fetch settings from server and merge (server wins) */
export async function fetchServerSettings(): Promise<void> {
  try {
    const serverSettings = await apiGet<Record<string, string>>('/api/user/settings');
    if (Object.keys(serverSettings).length === 0) return;

    userSettings.update((local) => {
      const merged = { ...local };
      for (const [key, value] of Object.entries(serverSettings)) {
        if (key in DEFAULT_SETTINGS && !LOCAL_ONLY_KEYS.has(key)) {
          // Type-coerce based on default type
          const defaultVal = DEFAULT_SETTINGS[key as keyof UserSettings];
          if (typeof defaultVal === 'number') {
            (merged as Record<string, unknown>)[key] = Number(value);
          } else if (typeof defaultVal === 'boolean') {
            (merged as Record<string, unknown>)[key] = value === 'true';
          } else {
            (merged as Record<string, unknown>)[key] = value;
          }
        }
      }
      return merged;
    });
  } catch (e) {
    console.warn('[settings] Failed to fetch server settings:', e);
  }
}

/** Handle a UserSettingsUpdate WebSocket broadcast */
export function handleUserSettingsUpdate(settings: Record<string, string>): void {
  userSettings.update((local) => {
    const merged = { ...local };
    for (const [key, value] of Object.entries(settings)) {
      if (key in DEFAULT_SETTINGS && !LOCAL_ONLY_KEYS.has(key)) {
        const defaultVal = DEFAULT_SETTINGS[key as keyof UserSettings];
        if (typeof defaultVal === 'number') {
          (merged as Record<string, unknown>)[key] = Number(value);
        } else if (typeof defaultVal === 'boolean') {
          (merged as Record<string, unknown>)[key] = value === 'true';
        } else {
          (merged as Record<string, unknown>)[key] = value;
        }
      }
    }
    return merged;
  });
}
