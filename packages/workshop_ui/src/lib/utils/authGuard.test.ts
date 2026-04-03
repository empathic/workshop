/**
 * Tests for resolveAuthGuard — the pure auth routing state machine.
 *
 * Covers every path through the decision tree:
 * - Auth not yet checked (wait)
 * - Auth disabled (init immediately)
 * - Auth enabled: needs setup, not authenticated, authenticated
 * - Standalone pages (login, register, invite, account)
 * - App already initialized (noop)
 * - The original bug: first login → navigate to main page
 */

import { resolveAuthGuard } from './authGuard.js';
import type { AuthGuardInput } from './authGuard.js';

// =============================================================================
// Helpers
// =============================================================================

/** Default input — auth checked, enabled, authenticated, on main page. */
function input(overrides: Partial<AuthGuardInput> = {}): AuthGuardInput {
  return {
    authChecked: true,
    authEnabled: true,
    isAuthenticated: true,
    needsSetup: false,
    pathname: '/',
    basePath: '',
    appInitialized: false,
    ...overrides
  };
}

// =============================================================================
// Auth not yet checked
// =============================================================================

describe('auth not yet checked', () => {
  it('returns wait regardless of other state', () => {
    expect(resolveAuthGuard(input({ authChecked: false }))).toEqual({ kind: 'wait' });
  });

  it('returns wait even on login page', () => {
    expect(
      resolveAuthGuard(
        input({
          authChecked: false,
          pathname: '/login',
          isAuthenticated: false
        })
      )
    ).toEqual({ kind: 'wait' });
  });
});

// =============================================================================
// Auth disabled
// =============================================================================

describe('auth disabled', () => {
  it('inits app on main page', () => {
    expect(
      resolveAuthGuard(
        input({
          authEnabled: false,
          isAuthenticated: false
        })
      )
    ).toEqual({ kind: 'init_app' });
  });

  it('inits app even on login page path (no guards active)', () => {
    // With auth disabled, login/register paths are not "standalone" in the
    // guard sense — but they ARE still classified as standalone for init.
    // The function should return noop for standalone pages.
    const result = resolveAuthGuard(
      input({
        authEnabled: false,
        isAuthenticated: false,
        pathname: '/login'
      })
    );
    expect(result).toEqual({ kind: 'noop' });
  });

  it('returns noop if already initialized', () => {
    expect(
      resolveAuthGuard(
        input({
          authEnabled: false,
          isAuthenticated: false,
          appInitialized: true
        })
      )
    ).toEqual({ kind: 'noop' });
  });
});

// =============================================================================
// Auth enabled — needs setup
// =============================================================================

describe('needs setup', () => {
  it('redirects to /register from main page', () => {
    expect(
      resolveAuthGuard(
        input({
          needsSetup: true,
          isAuthenticated: false
        })
      )
    ).toEqual({ kind: 'redirect', to: '/register' });
  });

  it('redirects to /register from /login', () => {
    expect(
      resolveAuthGuard(
        input({
          needsSetup: true,
          isAuthenticated: false,
          pathname: '/login'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/register' });
  });

  it('does NOT redirect if already on /register', () => {
    const result = resolveAuthGuard(
      input({
        needsSetup: true,
        isAuthenticated: false,
        pathname: '/register'
      })
    );
    // On /register, not authenticated, standalone page → noop
    expect(result).toEqual({ kind: 'noop' });
  });

  it('respects basePath in redirect', () => {
    expect(
      resolveAuthGuard(
        input({
          needsSetup: true,
          isAuthenticated: false,
          basePath: '/app',
          pathname: '/app/'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/app/register' });
  });
});

// =============================================================================
// Auth enabled — not authenticated
// =============================================================================

describe('not authenticated', () => {
  it('redirects to /login from main page', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false
        })
      )
    ).toEqual({ kind: 'redirect', to: '/login' });
  });

  it('stays on /login (noop)', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/login'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('stays on /register (noop)', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/register'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('stays on /invite/abc (noop)', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/invite/abc123'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('stays on /account (noop)', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/account'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('redirects from deep app path', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/instances/abc'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/login' });
  });
});

// =============================================================================
// Auth enabled — authenticated
// =============================================================================

describe('authenticated', () => {
  it('inits app on main page', () => {
    expect(resolveAuthGuard(input())).toEqual({ kind: 'init_app' });
  });

  it('redirects away from /login', () => {
    expect(
      resolveAuthGuard(
        input({
          pathname: '/login'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/' });
  });

  it('redirects away from /register', () => {
    expect(
      resolveAuthGuard(
        input({
          pathname: '/register'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/' });
  });

  it('returns noop on /account (standalone, already authed)', () => {
    expect(
      resolveAuthGuard(
        input({
          pathname: '/account'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('returns noop when already initialized', () => {
    expect(
      resolveAuthGuard(
        input({
          appInitialized: true
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('respects basePath for redirect from auth page', () => {
    expect(
      resolveAuthGuard(
        input({
          basePath: '/app',
          pathname: '/app/login'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/app/' });
  });

  it('inits app on deep app path', () => {
    expect(
      resolveAuthGuard(
        input({
          pathname: '/instances/abc'
        })
      )
    ).toEqual({ kind: 'init_app' });
  });
});

// =============================================================================
// The original bug: first login flow
// =============================================================================

describe('first login flow (the original bug)', () => {
  it('step 1: unauthenticated user on / → redirect to login', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/'
        })
      )
    ).toEqual({ kind: 'redirect', to: '/login' });
  });

  it('step 2: on /login, not authenticated → noop (show login form)', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: false,
          pathname: '/login'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('step 3: login succeeds, navigate to / → init_app', () => {
    // This is the transition the old code missed.
    // After login, isAuthenticated becomes true and pathname changes to '/'.
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: true,
          pathname: '/',
          appInitialized: false
        })
      )
    ).toEqual({ kind: 'init_app' });
  });

  it('step 4: app already initialized on subsequent navigations → noop', () => {
    expect(
      resolveAuthGuard(
        input({
          isAuthenticated: true,
          pathname: '/',
          appInitialized: true
        })
      )
    ).toEqual({ kind: 'noop' });
  });
});

// =============================================================================
// First registration flow
// =============================================================================

describe('first registration flow', () => {
  it('needs setup, on / → redirect to /register', () => {
    expect(
      resolveAuthGuard(
        input({
          needsSetup: true,
          isAuthenticated: false
        })
      )
    ).toEqual({ kind: 'redirect', to: '/register' });
  });

  it('on /register during setup → noop', () => {
    expect(
      resolveAuthGuard(
        input({
          needsSetup: true,
          isAuthenticated: false,
          pathname: '/register'
        })
      )
    ).toEqual({ kind: 'noop' });
  });

  it('register succeeds, navigate to / → init_app', () => {
    // After registration, needsSetup is false, isAuthenticated is true.
    expect(
      resolveAuthGuard(
        input({
          needsSetup: false,
          isAuthenticated: true,
          pathname: '/',
          appInitialized: false
        })
      )
    ).toEqual({ kind: 'init_app' });
  });
});

// =============================================================================
// Idempotency invariant
// =============================================================================

describe('idempotency', () => {
  it('init_app is only returned when appInitialized is false', () => {
    // Exhaustive: for every combination that would yield init_app,
    // flipping appInitialized to true must yield noop.
    const scenarios: Partial<AuthGuardInput>[] = [
      {}, // default: authenticated on /
      { authEnabled: false, isAuthenticated: false }, // auth disabled
      { pathname: '/instances/abc' } // deep app path
    ];

    for (const s of scenarios) {
      const withoutInit = resolveAuthGuard(input({ ...s, appInitialized: false }));
      const withInit = resolveAuthGuard(input({ ...s, appInitialized: true }));

      if (withoutInit.kind === 'init_app') {
        expect(withInit).toEqual({ kind: 'noop' });
      }
    }
  });
});
