/**
 * URL State Persistence
 *
 * Pure utility for syncing UI state to URL search params.
 * Used by instances, file explorer, and file viewer stores.
 */

export function updateUrl(params: Record<string, string | null>): void {
  const url = new URL(window.location.href);
  for (const [key, value] of Object.entries(params)) {
    if (value === null) {
      url.searchParams.delete(key);
    } else {
      url.searchParams.set(key, value);
    }
  }
  window.history.replaceState({}, '', url.toString());
}
