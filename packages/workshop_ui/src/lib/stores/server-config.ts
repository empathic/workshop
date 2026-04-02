import { writable, get } from 'svelte/store';
import { api, apiGet } from '$lib/utils/api';
import { setAuthEnabled } from '$lib/stores/auth';

// Re-export pure types and functions from the dependency-free utils module.
// Tests import from server-config-utils directly; components import from here.
export type { ServerConfig, OverrideState, ServerConfigState } from '$lib/utils/server-config-utils';
export {
  detectProfile,
  applyProfileDefaults,
  updateProfileAfterEdit,
  isDirty,
  isFieldDirty
} from '$lib/utils/server-config-utils';

import type { ServerConfig, OverrideState, ServerConfigState } from '$lib/utils/server-config-utils';
import { updateProfileAfterEdit } from '$lib/utils/server-config-utils';

// =============================================================================
// Internal types
// =============================================================================

interface ConfigApiResponse extends ServerConfig {
  overrides: OverrideState;
}

// =============================================================================
// Constants
// =============================================================================

const EMPTY_CONFIG: ServerConfig = {
  profile: null,
  host: '127.0.0.1',
  port: 8080,
  auth_enabled: false,
  https: false,
  scrollback_lines: 10000
};

const EMPTY_OVERRIDES: OverrideState = {
  host: false,
  port: false,
  auth_enabled: false,
  https: false,
  scrollback_lines: false
};

// =============================================================================
// Store
// =============================================================================

export const serverConfigState = writable<ServerConfigState>({
  server: null,
  local: { ...EMPTY_CONFIG },
  overrides: { ...EMPTY_OVERRIDES },
  loading: false,
  applying: false,
  error: null,
  statusMessage: null
});

// =============================================================================
// Actions
// =============================================================================

export async function fetchServerConfig(): Promise<void> {
  serverConfigState.update((s) => ({ ...s, loading: true, error: null }));

  try {
    const resp = await apiGet<ConfigApiResponse>('/api/admin/config');
    const server: ServerConfig = {
      profile: resp.profile,
      host: resp.host,
      port: resp.port,
      auth_enabled: resp.auth_enabled,
      https: resp.https,
      scrollback_lines: resp.scrollback_lines
    };
    serverConfigState.update((s) => ({
      ...s,
      server: { ...server },
      local: { ...server },
      overrides: { ...resp.overrides },
      loading: false
    }));
    // Auth-enabled is NOT written here. checkAuth() is the sole authoritative
    // source for auth state — this module only does optimistic writes in
    // applyConfig() for instant UI feedback before the server restarts.
  } catch (e) {
    serverConfigState.update((s) => ({
      ...s,
      loading: false,
      error: e instanceof Error ? e.message : 'Failed to load config'
    }));
  }
}

export function updateLocalField<K extends keyof ServerConfig>(key: K, value: ServerConfig[K]): void {
  serverConfigState.update((s) => {
    const updated = { ...s.local, [key]: value };
    return { ...s, local: updateProfileAfterEdit(updated), statusMessage: null };
  });
}

export function resetLocal(): void {
  serverConfigState.update((s) => ({
    ...s,
    local: s.server ? { ...s.server } : { ...EMPTY_CONFIG },
    statusMessage: null
  }));
}

export async function applyConfig(save: boolean): Promise<void> {
  const state = get(serverConfigState);
  if (!state.server) return;

  // Build diff-only patch body
  const body: Record<string, unknown> = { save };
  const fields: (keyof Omit<ServerConfig, 'profile'>)[] = ['host', 'port', 'auth_enabled', 'https', 'scrollback_lines'];
  for (const key of fields) {
    if (state.local[key] !== state.server[key]) {
      body[key] = state.local[key];
    }
  }

  // Nothing changed
  if (Object.keys(body).length === 1) return;

  serverConfigState.update((s) => ({ ...s, applying: true, error: null, statusMessage: null }));

  try {
    const resp = await api('/api/admin/config', {
      method: 'PATCH',
      body: JSON.stringify(body)
    });

    if (!resp.ok) {
      const data = await resp.json().catch(() => ({ error: 'Failed to apply config' }));
      serverConfigState.update((s) => ({
        ...s,
        applying: false,
        error: data.error || 'Failed to apply config'
      }));
      return;
    }

    serverConfigState.update((s) => ({
      ...s,
      applying: false,
      statusMessage: save ? 'Saved and applied' : 'Applied (ephemeral)'
    }));

    // Eagerly update auth store so the UI reflects the change immediately
    // (AdminTab, auth guard) without waiting for the WS reconnect re-sync.
    if (body.auth_enabled !== undefined) {
      setAuthEnabled(body.auth_enabled as boolean);
    }

    // Authoritative re-sync happens automatically when the WS reconnects
    // after server restart (see websocket.ts onopen handler).
  } catch (e) {
    serverConfigState.update((s) => ({
      ...s,
      applying: false,
      error: e instanceof Error ? e.message : 'Network error'
    }));
  }
}
