/**
 * Shared instance state utilities
 *
 * Extracted from Sidebar.svelte so both the Sidebar/rail and InstanceChip
 * can compute instance state display info.
 */

import type { ClaudeState } from '$lib/types';
import { getInstanceVerb } from '$lib/stores/activity';

export interface StateInfo {
  label: string;
  color: string;
  animate: boolean;
  stale: boolean;
}

export function getStateInfo(instanceId: string, state: ClaudeState | undefined, stale: boolean = false): StateInfo {
  if (!state) {
    return { label: '', color: 'var(--text-muted)', animate: false, stale: false };
  }

  switch (state.type) {
    case 'Initializing':
      return { label: 'init', color: 'var(--text-muted)', animate: true, stale: false };
    case 'Starting':
      return { label: 'starting', color: 'var(--accent-500)', animate: true, stale: false };
    case 'Idle':
      return { label: '', color: 'var(--status-green)', animate: false, stale: false };
    case 'Thinking': {
      const verb = getInstanceVerb(instanceId, 'Thinking').toLowerCase();
      return { label: stale ? `${verb}?` : verb, color: 'var(--thinking-500)', animate: !stale, stale };
    }
    case 'Responding': {
      const verb = getInstanceVerb(instanceId, 'Responding').toLowerCase();
      return { label: stale ? `${verb}?` : verb, color: 'var(--accent-500)', animate: !stale, stale };
    }
    case 'ToolExecuting':
      return { label: stale ? `${state.tool}?` : state.tool, color: 'var(--accent-400)', animate: !stale, stale };
    case 'WaitingForInput':
      return { label: 'ready', color: 'var(--status-green)', animate: false, stale: false };
    default:
      return { label: '', color: 'var(--text-muted)', animate: false, stale: false };
  }
}
