/**
 * Authentication Store
 *
 * Single source of truth for auth state. All fields live in one writable;
 * read-only derived accessors preserve the same $store API for consumers.
 */

import { writable, derived } from 'svelte/store';
import { api } from '$lib/utils/api';

// =============================================================================
// Types
// =============================================================================

export interface AuthUser {
  id: string;
  username: string;
  display_name: string;
  is_admin: boolean;
}

interface AuthState {
  user: AuthUser | null;
  csrfToken: string | null;
  enabled: boolean;
  needsSetup: boolean;
  checked: boolean;
}

interface MeResponse {
  user?: AuthUser;
  csrf_token?: string;
  needs_setup: boolean;
  auth_enabled: boolean;
}

interface AuthResponse {
  user: AuthUser;
  csrf_token: string;
}

// =============================================================================
// Stores
// =============================================================================

/**
 * Private single source of truth. Mixes session fields (user, csrfToken) with
 * server config (enabled, needsSetup) because they arrive together from
 * /api/auth/me and must transition atomically (e.g. enabling auth clears the
 * session in the same tick). checkAuth() is the sole authoritative writer for
 * all fields; setAuthEnabled() exists only for optimistic UI in applyConfig().
 */
const authState = writable<AuthState>({
  user: null,
  csrfToken: null,
  enabled: false,
  needsSetup: false,
  checked: false
});

/** Read-only derived accessors — same $store syntax, no consumer changes */
export const currentUser = derived(authState, ($s) => $s.user);
export const csrfToken = derived(authState, ($s) => $s.csrfToken);
export const authEnabled = derived(authState, ($s) => $s.enabled);
export const needsSetup = derived(authState, ($s) => $s.needsSetup);
export const authChecked = derived(authState, ($s) => $s.checked);

export const isAuthenticated = derived(authState, ($s) => $s.user !== null);

// =============================================================================
// Mutation Functions
// =============================================================================

/** Update auth-enabled flag from server-config or other external source. */
export function setAuthEnabled(enabled: boolean): void {
  authState.update((s) => ({ ...s, enabled }));
}

/** Clear user session (on 401, logout, etc.) */
export function clearSession(): void {
  authState.update((s) => ({ ...s, user: null, csrfToken: null }));
}

// =============================================================================
// Functions
// =============================================================================

/**
 * Check current auth status by calling GET /api/auth/me.
 * Returns the auth state for routing decisions.
 */
export async function checkAuth(): Promise<{
  authenticated: boolean;
  needsSetup: boolean;
  authEnabled: boolean;
}> {
  try {
    const response = await fetch('/api/auth/me', {
      credentials: 'same-origin'
    });
    if (!response.ok) throw new Error('Auth check failed');

    const data: MeResponse = await response.json();

    authState.update((s) => ({
      ...s,
      enabled: data.auth_enabled,
      needsSetup: data.needs_setup,
      user: data.user ?? null,
      csrfToken: data.user ? (data.csrf_token ?? null) : null,
      checked: true
    }));

    return {
      authenticated: !!data.user,
      needsSetup: data.needs_setup,
      authEnabled: data.auth_enabled
    };
  } catch (error) {
    console.error('Auth check failed:', error);
    authState.update((s) => ({ ...s, checked: true }));
    return { authenticated: false, needsSetup: false, authEnabled: false };
  }
}

/**
 * Log in with username and password.
 */
export async function login(username: string, password: string): Promise<{ ok: boolean; error?: string }> {
  try {
    const response = await fetch('/api/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'same-origin',
      body: JSON.stringify({ username, password })
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({ error: 'Login failed' }));
      return { ok: false, error: data.error || 'Login failed' };
    }

    const data: AuthResponse = await response.json();
    authState.update((s) => ({ ...s, user: data.user, csrfToken: data.csrf_token }));
    return { ok: true };
  } catch (error) {
    return { ok: false, error: 'Network error' };
  }
}

/**
 * Register a new user.
 */
export async function register(
  username: string,
  password: string,
  displayName?: string,
  inviteToken?: string
): Promise<{ ok: boolean; error?: string }> {
  try {
    const body: Record<string, string | undefined> = {
      username,
      password,
      display_name: displayName,
      invite_token: inviteToken
    };
    const response = await fetch('/api/auth/register', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      credentials: 'same-origin',
      body: JSON.stringify(body)
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({ error: 'Registration failed' }));
      return { ok: false, error: data.error || 'Registration failed' };
    }

    const data: AuthResponse = await response.json();
    authState.update((s) => ({ ...s, user: data.user, csrfToken: data.csrf_token }));
    return { ok: true };
  } catch (error) {
    return { ok: false, error: 'Network error' };
  }
}

/**
 * Change the current user's password.
 */
export async function changePassword(
  currentPassword: string,
  newPassword: string
): Promise<{ ok: boolean; error?: string }> {
  try {
    const response = await api('/api/auth/change-password', {
      method: 'POST',
      body: JSON.stringify({ current_password: currentPassword, new_password: newPassword })
    });

    if (!response.ok) {
      const data = await response.json().catch(() => ({ error: 'Failed to change password' }));
      return { ok: false, error: data.error || 'Failed to change password' };
    }

    return { ok: true };
  } catch (error) {
    return { ok: false, error: 'Network error' };
  }
}

/**
 * Log out the current user.
 */
export async function logout(): Promise<void> {
  try {
    await api('/api/auth/logout', { method: 'POST' });
  } catch {
    // Ignore errors - we'll clear state anyway
  }
  clearSession();
}
