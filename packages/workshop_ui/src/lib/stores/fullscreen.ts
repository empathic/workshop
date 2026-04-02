import { writable, derived } from 'svelte/store';

export type FullscreenView = 'history' | 'settings' | 'new-project' | null;

const _fullscreenView = writable<FullscreenView>(null);
export const fullscreenView = { subscribe: _fullscreenView.subscribe };
export const isFullscreenOpen = derived(_fullscreenView, (v) => v !== null);

export function openFullscreen(view: FullscreenView) {
  _fullscreenView.set(view);
}
export function closeFullscreen() {
  _fullscreenView.set(null);
}
