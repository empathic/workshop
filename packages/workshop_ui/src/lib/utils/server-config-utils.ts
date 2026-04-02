// Pure functions and types for server config.
// Extracted so they can be tested without Svelte/store dependencies.

// =============================================================================
// Types
// =============================================================================

export interface ServerConfig {
  profile: string | null;
  host: string;
  port: number;
  auth_enabled: boolean;
  https: boolean;
  scrollback_lines: number;
}

export interface OverrideState {
  host: boolean;
  port: boolean;
  auth_enabled: boolean;
  https: boolean;
  scrollback_lines: boolean;
}

export interface ServerConfigState {
  server: ServerConfig | null;
  local: ServerConfig;
  overrides: OverrideState;
  loading: boolean;
  applying: boolean;
  error: string | null;
  statusMessage: string | null;
}

// =============================================================================
// Profile Detection (ported from cli/settings.rs:420-430)
// =============================================================================

export function detectProfile(config: ServerConfig): string | null {
  if (config.host === '127.0.0.1' && !config.auth_enabled && !config.https) {
    return 'local';
  }
  if (config.host === '127.0.0.1' && config.auth_enabled && config.https) {
    return 'tunnel';
  }
  if (config.host === '0.0.0.0' && config.auth_enabled && config.https) {
    return 'server';
  }
  return null;
}

const PROFILE_DEFAULTS: Record<string, { host: string; auth_enabled: boolean; https: boolean }> = {
  local: { host: '127.0.0.1', auth_enabled: false, https: false },
  tunnel: { host: '127.0.0.1', auth_enabled: true, https: true },
  server: { host: '0.0.0.0', auth_enabled: true, https: true }
};

export function applyProfileDefaults(config: ServerConfig, profile: string): ServerConfig {
  const defaults = PROFILE_DEFAULTS[profile];
  if (!defaults) return config;
  return {
    ...config,
    profile,
    host: defaults.host,
    auth_enabled: defaults.auth_enabled,
    https: defaults.https
  };
}

/** After editing a field, re-detect the profile. */
export function updateProfileAfterEdit(config: ServerConfig): ServerConfig {
  return { ...config, profile: detectProfile(config) };
}

// =============================================================================
// Dirty tracking
// =============================================================================

/** Check if any local field differs from the server snapshot. */
export function isDirty(state: ServerConfigState): boolean {
  if (!state.server) return false;
  return (
    state.local.host !== state.server.host ||
    state.local.port !== state.server.port ||
    state.local.auth_enabled !== state.server.auth_enabled ||
    state.local.https !== state.server.https ||
    state.local.scrollback_lines !== state.server.scrollback_lines
  );
}

/** Check if a specific field is dirty. */
export function isFieldDirty(state: ServerConfigState, key: keyof ServerConfig): boolean {
  if (!state.server) return false;
  return state.local[key] !== state.server[key];
}
