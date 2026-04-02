/**
 * UI State Store
 *
 * Manages responsive UI state
 */

import { writable, derived } from 'svelte/store';
import { browser } from '$app/environment';

// Breakpoints matching CSS media queries
export const BREAKPOINTS = {
  mobile: 640,
  tablet: 1024
} as const;

/** Current viewport width */
export const viewportWidth = writable<number>(browser ? window.innerWidth : 1200);

/** Whether we're on a mobile viewport */
export const isMobile = derived(viewportWidth, ($w) => $w < BREAKPOINTS.mobile);

/** Whether we're on a tablet viewport */
export const isTablet = derived(viewportWidth, ($w) => $w >= BREAKPOINTS.mobile && $w < BREAKPOINTS.tablet);

/** Whether we're on desktop */
export const isDesktop = derived(viewportWidth, ($w) => $w >= BREAKPOINTS.tablet);

// Initialize viewport listener
if (browser) {
  const updateWidth = () => viewportWidth.set(window.innerWidth);
  window.addEventListener('resize', updateWidth);
}
