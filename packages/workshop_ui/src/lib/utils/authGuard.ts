/**
 * Pure auth guard + app initialization state machine.
 *
 * Given the current auth state and route, returns the action the layout
 * should take. Extracted from the +layout.svelte $effect so it can be
 * unit tested without a component harness.
 */

// =============================================================================
// Types
// =============================================================================

export interface AuthGuardInput {
  authChecked: boolean;
  authEnabled: boolean;
  isAuthenticated: boolean;
  needsSetup: boolean;
  pathname: string;
  basePath: string;
  appInitialized: boolean;
}

export type AuthGuardAction =
  | { kind: 'wait' }
  | { kind: 'redirect'; to: string }
  | { kind: 'init_app' }
  | { kind: 'noop' };

// =============================================================================
// Helpers
// =============================================================================

function stripBase(pathname: string, basePath: string): string {
  return pathname.replace(basePath, '') || '/';
}

function isStandalone(path: string): boolean {
  return path === '/login' || path === '/register' || path.startsWith('/invite') || path === '/account';
}

// =============================================================================
// State machine
// =============================================================================

export function resolveAuthGuard(input: AuthGuardInput): AuthGuardAction {
  if (!input.authChecked) return { kind: 'wait' };

  const path = stripBase(input.pathname, input.basePath);
  const standalone = isStandalone(path);
  const isAuthPage = path === '/login' || path === '/register';

  if (input.authEnabled) {
    if (input.needsSetup && path !== '/register') {
      return { kind: 'redirect', to: `${input.basePath}/register` };
    }
    if (!input.isAuthenticated && !standalone) {
      return { kind: 'redirect', to: `${input.basePath}/login` };
    }
    if (input.isAuthenticated && isAuthPage) {
      return { kind: 'redirect', to: `${input.basePath}/` };
    }
  }

  if (!standalone && !input.appInitialized && (!input.authEnabled || input.isAuthenticated)) {
    return { kind: 'init_app' };
  }

  return { kind: 'noop' };
}
