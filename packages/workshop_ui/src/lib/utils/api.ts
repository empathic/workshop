/**
 * API utility that adds CSRF token and credentials to requests.
 * Intercepts 401 responses to clear stale auth state and redirect to login.
 */

import { get } from 'svelte/store';
import { base } from '$app/paths';
import { csrfToken, authEnabled, clearSession } from '$lib/stores/auth';

const MUTATION_METHODS = ['POST', 'PUT', 'DELETE', 'PATCH'];

/**
 * Handle 401 by clearing auth state and redirecting to login.
 * Skips redirect if auth is disabled or already on an auth page.
 */
function handleUnauthorized(): void {
  if (!get(authEnabled)) return;

  const path = window.location.pathname;
  if (path === `${base}/login` || path === `${base}/register`) return;

  clearSession();
  window.location.href = `${base}/login`;
}

/**
 * Fetch wrapper that:
 * - Sets credentials: 'same-origin' for cookie auth
 * - Adds X-CSRF-Token header on mutation requests
 * - Sets Content-Type to application/json for mutations with body
 * - Intercepts 401 responses to clear stale auth and redirect
 */
export async function api(path: string, options: RequestInit = {}): Promise<Response> {
  const method = (options.method || 'GET').toUpperCase();
  const headers = new Headers(options.headers);

  // Add CSRF token for mutations
  if (MUTATION_METHODS.includes(method)) {
    const token = get(csrfToken);
    if (token) {
      headers.set('X-CSRF-Token', token);
    }
  }

  // Set content type if we have a body and it's not already set
  if (options.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const response = await fetch(path, {
    ...options,
    headers,
    credentials: 'same-origin'
  });

  // Intercept 401: session expired or invalid
  if (response.status === 401 && !path.startsWith('/api/auth/')) {
    handleUnauthorized();
  }

  return response;
}

/**
 * Convenience: GET JSON from API.
 */
export async function apiGet<T>(path: string): Promise<T> {
  const response = await api(path);
  if (!response.ok) throw new Error(`API error: ${response.status}`);
  return response.json();
}

/**
 * Convenience: POST JSON to API.
 */
export async function apiPost<T>(path: string, body?: unknown): Promise<T> {
  const response = await api(path, {
    method: 'POST',
    body: body ? JSON.stringify(body) : undefined
  });
  if (!response.ok) throw new Error(`API error: ${response.status}`);
  return response.json();
}

/**
 * Convenience: DELETE to API.
 */
export async function apiDelete(path: string): Promise<void> {
  const response = await api(path, { method: 'DELETE' });
  if (!response.ok) throw new Error(`API error: ${response.status}`);
}
