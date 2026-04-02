import {
  detectProfile,
  applyProfileDefaults,
  isDirty,
  isFieldDirty,
  type ServerConfig,
  type ServerConfigState,
  type OverrideState
} from './server-config-utils.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeConfig(overrides: Partial<ServerConfig> = {}): ServerConfig {
  return {
    profile: null,
    host: '127.0.0.1',
    port: 8080,
    auth_enabled: false,
    https: false,
    scrollback_lines: 10000,
    ...overrides
  };
}

function makeOverrides(overrides: Partial<OverrideState> = {}): OverrideState {
  return {
    host: false,
    port: false,
    auth_enabled: false,
    https: false,
    scrollback_lines: false,
    ...overrides
  };
}

function makeState(overrides: Partial<ServerConfigState> = {}): ServerConfigState {
  return {
    server: makeConfig(),
    local: makeConfig(),
    overrides: makeOverrides(),
    loading: false,
    applying: false,
    error: null,
    statusMessage: null,
    ...overrides
  };
}

// ---------------------------------------------------------------------------
// detectProfile
// ---------------------------------------------------------------------------

describe('detectProfile', () => {
  it('detects local profile', () => {
    expect(detectProfile(makeConfig({ host: '127.0.0.1', auth_enabled: false, https: false }))).toBe('local');
  });

  it('detects tunnel profile', () => {
    expect(detectProfile(makeConfig({ host: '127.0.0.1', auth_enabled: true, https: true }))).toBe('tunnel');
  });

  it('detects server profile', () => {
    expect(detectProfile(makeConfig({ host: '0.0.0.0', auth_enabled: true, https: true }))).toBe('server');
  });

  it('returns null for non-matching config', () => {
    expect(detectProfile(makeConfig({ host: '0.0.0.0', auth_enabled: false, https: false }))).toBeNull();
  });

  it('returns null when auth enabled but not https', () => {
    expect(detectProfile(makeConfig({ host: '127.0.0.1', auth_enabled: true, https: false }))).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// applyProfileDefaults
// ---------------------------------------------------------------------------

describe('applyProfileDefaults', () => {
  it('applies local defaults', () => {
    const base = makeConfig({ host: '0.0.0.0', auth_enabled: true, https: true, port: 9090 });
    const result = applyProfileDefaults(base, 'local');
    expect(result.host).toBe('127.0.0.1');
    expect(result.auth_enabled).toBe(false);
    expect(result.https).toBe(false);
    expect(result.profile).toBe('local');
    // Preserves non-profile fields
    expect(result.port).toBe(9090);
    expect(result.scrollback_lines).toBe(10000);
  });

  it('applies tunnel defaults', () => {
    const result = applyProfileDefaults(makeConfig(), 'tunnel');
    expect(result.host).toBe('127.0.0.1');
    expect(result.auth_enabled).toBe(true);
    expect(result.https).toBe(true);
    expect(result.profile).toBe('tunnel');
  });

  it('applies server defaults', () => {
    const result = applyProfileDefaults(makeConfig(), 'server');
    expect(result.host).toBe('0.0.0.0');
    expect(result.auth_enabled).toBe(true);
    expect(result.https).toBe(true);
    expect(result.profile).toBe('server');
  });

  it('returns config unchanged for unknown profile', () => {
    const base = makeConfig({ port: 3000 });
    const result = applyProfileDefaults(base, 'unknown');
    expect(result).toEqual(base);
  });

  it('does not mutate the input config', () => {
    const base = makeConfig();
    const original = { ...base };
    applyProfileDefaults(base, 'server');
    expect(base).toEqual(original);
  });
});

// ---------------------------------------------------------------------------
// isDirty
// ---------------------------------------------------------------------------

describe('isDirty', () => {
  it('returns false when local matches server', () => {
    expect(isDirty(makeState())).toBe(false);
  });

  it('returns false when server is null', () => {
    expect(isDirty(makeState({ server: null }))).toBe(false);
  });

  it('detects host change', () => {
    expect(isDirty(makeState({ local: makeConfig({ host: '0.0.0.0' }) }))).toBe(true);
  });

  it('detects port change', () => {
    expect(isDirty(makeState({ local: makeConfig({ port: 9090 }) }))).toBe(true);
  });

  it('detects auth_enabled change', () => {
    expect(isDirty(makeState({ local: makeConfig({ auth_enabled: true }) }))).toBe(true);
  });

  it('detects https change', () => {
    expect(isDirty(makeState({ local: makeConfig({ https: true }) }))).toBe(true);
  });

  it('detects scrollback_lines change', () => {
    expect(isDirty(makeState({ local: makeConfig({ scrollback_lines: 500 }) }))).toBe(true);
  });

  it('ignores profile-only change', () => {
    // Profile is derived, not a user-editable field — isDirty should not flag it
    expect(isDirty(makeState({ local: makeConfig({ profile: 'server' }) }))).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// isFieldDirty
// ---------------------------------------------------------------------------

describe('isFieldDirty', () => {
  it('returns false when server is null', () => {
    expect(isFieldDirty(makeState({ server: null }), 'host')).toBe(false);
  });

  it('returns false when field matches', () => {
    expect(isFieldDirty(makeState(), 'host')).toBe(false);
  });

  it('returns true when field differs', () => {
    expect(isFieldDirty(makeState({ local: makeConfig({ port: 3000 }) }), 'port')).toBe(true);
  });

  it('only flags the changed field', () => {
    const state = makeState({ local: makeConfig({ auth_enabled: true }) });
    expect(isFieldDirty(state, 'auth_enabled')).toBe(true);
    expect(isFieldDirty(state, 'host')).toBe(false);
    expect(isFieldDirty(state, 'port')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Auth gate: profile presets that enable auth
// ---------------------------------------------------------------------------

describe('profile presets and auth gate', () => {
  it('tunnel preset enables auth', () => {
    const result = applyProfileDefaults(makeConfig(), 'tunnel');
    expect(result.auth_enabled).toBe(true);
  });

  it('server preset enables auth', () => {
    const result = applyProfileDefaults(makeConfig(), 'server');
    expect(result.auth_enabled).toBe(true);
  });

  it('local preset disables auth', () => {
    const result = applyProfileDefaults(makeConfig({ auth_enabled: true }), 'local');
    expect(result.auth_enabled).toBe(false);
  });

  it('switching from server to local marks auth_enabled as dirty then clean', () => {
    const server = makeConfig(); // defaults: auth_enabled: false
    const afterServer = applyProfileDefaults(server, 'server');
    const stateAfterServer = makeState({ server, local: afterServer });
    expect(isFieldDirty(stateAfterServer, 'auth_enabled')).toBe(true);

    const afterLocal = applyProfileDefaults(afterServer, 'local');
    const stateAfterLocal = makeState({ server, local: afterLocal });
    expect(isFieldDirty(stateAfterLocal, 'auth_enabled')).toBe(false);
  });
});
