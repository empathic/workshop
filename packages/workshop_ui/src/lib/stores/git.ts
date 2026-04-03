/**
 * Git Store
 *
 * Manages git state for the current instance: log, branches, status, diff.
 * Unidirectional data flow — all mutations through explicit actions.
 */

import { writable, derived, get } from 'svelte/store';
import { currentInstance, updateUrl } from './instances';
import { apiGet } from '$lib/utils/api';

// =============================================================================
// Types
// =============================================================================

export interface GitCommit {
  hash: string;
  shortHash: string;
  authorName: string;
  authorEmail: string;
  date: number;
  message: string;
  body: string;
  refs: string[];
}

export interface GitBranch {
  name: string;
  current: boolean;
  remote: boolean;
  lastCommitHash: string;
  lastCommitDate: number;
  lastCommitMessage: string;
  upstream: string | null;
  ahead: number;
  behind: number;
}

export interface InstanceBranchInfo {
  instance_id: string;
  instance_name: string;
  branch: string;
}

export interface GitFileStatus {
  path: string;
  status: string;
  oldPath?: string;
}

export interface GitStatusData {
  branch: string;
  staged: GitFileStatus[];
  unstaged: GitFileStatus[];
  untracked: GitFileStatus[];
  aheadBehind: [number, number] | null;
}

export interface InlineHighlight {
  start: number;
  end: number;
}

export interface GitDiffLine {
  type: 'add' | 'del' | 'ctx';
  content: string;
  oldNum?: number;
  newNum?: number;
  highlights?: InlineHighlight[];
}

export interface GitDiffHunk {
  header: string;
  lines: GitDiffLine[];
}

export interface GitDiffFile {
  path: string;
  oldPath?: string;
  status: string;
  additions: number;
  deletions: number;
  hunks: GitDiffHunk[];
}

export interface GitDiffData {
  files: GitDiffFile[];
  stats: { additions: number; deletions: number; filesChanged: number };
  /** Which engine actually produced the diff. Present when structural was requested —
   *  "structural" if it succeeded, "patience" if it fell back. */
  engine?: string;
}

// =============================================================================
// Stores
// =============================================================================

export const isGitOpen = writable<boolean>(false);
export const gitTab = writable<'log' | 'branches' | 'status'>('status');
export const gitCommits = writable<GitCommit[]>([]);
export const gitBranches = writable<GitBranch[]>([]);
export const gitCurrentBranch = writable<string>('');
export const gitDefaultBranch = writable<string | null>(null);
export const gitInstanceBranches = writable<InstanceBranchInfo[]>([]);
export const gitStatus = writable<GitStatusData | null>(null);
export const gitDiff = writable<GitDiffData | null>(null);
export const gitDiffTarget = writable<string | null>(null);
export const gitHasMore = writable<boolean>(false);
export const gitLoading = writable<boolean>(false);
export const gitError = writable<string | null>(null);
export const selectedCommitHash = writable<string | null>(null);

// Branch expansion state
export const selectedBranchName = writable<string | null>(null);
export const branchLogPreview = writable<GitCommit[]>([]);
export const branchDiff = writable<GitDiffData | null>(null);
export const branchDiffLoading = writable<boolean>(false);
export const branchDiffMode = writable<'threedot' | 'twodot'>('threedot');
/** The branch being compared against (default: upstream or current branch) */
export const branchDiffBase = writable<string | null>(null);
export const logBranchFilter = writable<string | null>(null);

/** Map of file path → git status for file explorer badges */
export const gitFileStatuses = writable<Map<string, string>>(new Map());

// =============================================================================
// Derived
// =============================================================================

export const statusCounts = derived(gitStatus, ($status) => {
  if (!$status) return { staged: 0, modified: 0, untracked: 0, total: 0 };
  const staged = $status.staged.length;
  const modified = $status.unstaged.length;
  const untracked = $status.untracked.length;
  return { staged, modified, untracked, total: staged + modified + untracked };
});

export const currentBranchInfo = derived(
  [gitBranches, gitCurrentBranch],
  ([$branches, $current]) => $branches.find((b) => b.name === $current) ?? null
);

/** Map of directory path (relative) → count of changed files under it */
export const directoryVcsStatuses = derived(gitFileStatuses, ($statuses) => {
  const dirCounts = new Map<string, number>();
  for (const filePath of $statuses.keys()) {
    // Walk path segments upward: "src/lib/foo.ts" → "src/lib", "src"
    const parts = filePath.split('/');
    for (let i = parts.length - 1; i >= 1; i--) {
      const dir = parts.slice(0, i).join('/');
      dirCounts.set(dir, (dirCounts.get(dir) ?? 0) + 1);
    }
  }
  return dirCounts;
});

// =============================================================================
// Actions
// =============================================================================

export async function fetchGitLog(
  instanceId: string,
  opts?: { limit?: number; offset?: number; branch?: string }
): Promise<void> {
  gitLoading.set(true);
  gitError.set(null);
  try {
    const params = new URLSearchParams();
    if (opts?.limit) params.set('limit', String(opts.limit));
    if (opts?.offset) params.set('offset', String(opts.offset));
    if (opts?.branch) params.set('branch', opts.branch);
    const qs = params.toString();
    const url = `/api/instances/${instanceId}/git/log${qs ? '?' + qs : ''}`;
    const data = await apiGet<{ commits: GitCommit[]; hasMore: boolean }>(url);

    if (opts?.offset && opts.offset > 0) {
      // Append to existing commits (pagination)
      gitCommits.update((prev) => [...prev, ...data.commits]);
    } else {
      gitCommits.set(data.commits);
    }
    gitHasMore.set(data.hasMore);
  } catch (e) {
    gitError.set(e instanceof Error ? e.message : 'Failed to fetch git log');
  } finally {
    gitLoading.set(false);
  }
}

export async function fetchGitBranches(instanceId: string): Promise<void> {
  gitLoading.set(true);
  gitError.set(null);
  try {
    const data = await apiGet<{
      branches: GitBranch[];
      current: string;
      defaultBranch?: string;
      instanceBranches: InstanceBranchInfo[];
    }>(`/api/instances/${instanceId}/git/branches`);
    gitBranches.set(data.branches);
    gitCurrentBranch.set(data.current);
    gitDefaultBranch.set(data.defaultBranch ?? null);
    gitInstanceBranches.set(data.instanceBranches);
  } catch (e) {
    gitError.set(e instanceof Error ? e.message : 'Failed to fetch branches');
  } finally {
    gitLoading.set(false);
  }
}

export async function fetchGitStatus(instanceId: string): Promise<void> {
  gitError.set(null);
  try {
    const data = await apiGet<GitStatusData>(`/api/instances/${instanceId}/git/status`);
    gitStatus.set(data);
    refreshFileStatuses(data);
  } catch (e) {
    gitError.set(e instanceof Error ? e.message : 'Failed to fetch git status');
  }
}

export async function fetchGitDiff(
  instanceId: string,
  commit?: string,
  path?: string,
  engine?: 'standard' | 'patience' | 'structural',
  opts?: { base?: string; head?: string; diffMode?: 'twodot' | 'threedot'; statOnly?: boolean }
): Promise<void> {
  gitLoading.set(true);
  gitError.set(null);
  try {
    const data = await fetchDiffDirect(instanceId, commit, path, engine, opts);
    gitDiff.set(data);
    gitDiffTarget.set(commit ?? null);
  } catch (e) {
    gitError.set(e instanceof Error ? e.message : 'Failed to fetch diff');
  } finally {
    gitLoading.set(false);
  }
}

/**
 * Fetch diff data directly — returns the result without writing to any store.
 * Use this in reactive effects to avoid stomping the global gitDiff store
 * when multiple fetches race.
 */
export async function fetchDiffDirect(
  instanceId: string,
  commit?: string,
  path?: string,
  engine?: 'standard' | 'patience' | 'structural',
  opts?: { base?: string; head?: string; diffMode?: 'twodot' | 'threedot'; statOnly?: boolean }
): Promise<GitDiffData> {
  const params = new URLSearchParams();
  if (commit) params.set('commit', commit);
  if (path) params.set('path', path);
  if (engine) params.set('engine', engine);
  if (opts?.base) params.set('base', opts.base);
  if (opts?.head) params.set('head', opts.head);
  if (opts?.diffMode) params.set('diff_mode', opts.diffMode);
  if (opts?.statOnly) params.set('stat_only', 'true');
  const qs = params.toString();
  return apiGet<GitDiffData>(`/api/instances/${instanceId}/git/diff${qs ? '?' + qs : ''}`);
}

export function selectCommit(hash: string | null): void {
  selectedCommitHash.set(hash);
  if (hash) {
    gitDiff.set(null); // Clear stale diff while loading
    const instance = get(currentInstance);
    if (instance) {
      fetchGitDiff(instance.id, hash);
    }
  } else {
    gitDiff.set(null);
  }
}

// =============================================================================
// Branch expansion actions
// =============================================================================

/** Resolve the default comparison base: upstream → remote default branch → current branch. */
function resolveDefaultBase(branchName: string): string {
  const branches = get(gitBranches);
  const branch = branches.find((b) => b.name === branchName);
  if (branch?.upstream) {
    return branch.upstream;
  }
  // Use the remote's default branch (from origin/HEAD)
  const defaultBranch = get(gitDefaultBranch);
  if (defaultBranch && defaultBranch !== branchName) {
    // Prefer the local branch if it exists, otherwise use origin/name
    const branchNames = new Set(branches.map((b) => b.name));
    if (branchNames.has(defaultBranch)) return defaultBranch;
    const originRef = `origin/${defaultBranch}`;
    if (branchNames.has(originRef)) return originRef;
  }
  return get(gitCurrentBranch);
}

export async function selectBranch(name: string): Promise<void> {
  const current = get(selectedBranchName);
  if (current === name) {
    // Collapse
    selectedBranchName.set(null);
    branchLogPreview.set([]);
    branchDiff.set(null);
    branchDiffBase.set(null);
    return;
  }

  selectedBranchName.set(name);
  branchDiffLoading.set(true);
  branchDiff.set(null);
  branchLogPreview.set([]);

  const instance = get(currentInstance);
  if (!instance) return;

  const base = resolveDefaultBase(name);
  branchDiffBase.set(base);
  const mode = get(branchDiffMode);

  try {
    // Fetch log preview and diff summary in parallel
    const logPromise = apiGet<{ commits: GitCommit[]; hasMore: boolean }>(
      `/api/instances/${instance.id}/git/log?limit=5&branch=${encodeURIComponent(name)}`
    );

    const diffPromise =
      name !== base
        ? apiGet<GitDiffData>(
            `/api/instances/${instance.id}/git/diff?base=${encodeURIComponent(base)}&head=${encodeURIComponent(name)}&diff_mode=${mode}&stat_only=true`
          )
        : Promise.resolve(null);

    const [logData, diffData] = await Promise.all([logPromise, diffPromise]);

    // Guard against stale responses
    if (get(selectedBranchName) !== name) return;

    branchLogPreview.set(logData.commits);
    if (diffData) branchDiff.set(diffData);
  } catch (e) {
    console.error('Failed to expand branch:', e);
  } finally {
    branchDiffLoading.set(false);
  }
}

/** Change the base branch for comparison and re-fetch the diff. */
export async function changeBranchDiffBase(newBase: string): Promise<void> {
  branchDiffBase.set(newBase);
  await refetchBranchDiff();
}

export async function refetchBranchDiff(): Promise<void> {
  const name = get(selectedBranchName);
  if (!name) return;

  const instance = get(currentInstance);
  if (!instance) return;

  const base = get(branchDiffBase);
  if (!base || name === base) {
    branchDiff.set(null);
    return;
  }

  const mode = get(branchDiffMode);
  branchDiffLoading.set(true);

  try {
    const diffData = await apiGet<GitDiffData>(
      `/api/instances/${instance.id}/git/diff?base=${encodeURIComponent(base)}&head=${encodeURIComponent(name)}&diff_mode=${mode}&stat_only=true`
    );
    if (get(selectedBranchName) !== name) return;
    branchDiff.set(diffData);
  } catch (e) {
    console.error('Failed to refetch branch diff:', e);
  } finally {
    branchDiffLoading.set(false);
  }
}

export function viewBranchLog(name: string): void {
  logBranchFilter.set(name);
  gitTab.set('log');
}

export function clearLogBranchFilter(): void {
  logBranchFilter.set(null);
}

export function openGitTab(): void {
  isGitOpen.set(true);
  updateUrl({ explorer: 'git' });
}

export function closeGitTab(): void {
  isGitOpen.set(false);
  updateUrl({ explorer: 'files' });
}

/** Build the gitFileStatuses map from a status response for O(1) badge lookups */
export function refreshFileStatuses(status: GitStatusData): void {
  const map = new Map<string, string>();
  for (const f of status.staged) {
    map.set(f.path, f.status === 'added' ? 'A' : f.status === 'deleted' ? 'D' : f.status === 'renamed' ? 'R' : 'M');
  }
  for (const f of status.unstaged) {
    // Don't overwrite staged status — staged takes priority in badge
    if (!map.has(f.path)) {
      map.set(f.path, 'M');
    }
  }
  for (const f of status.untracked) {
    map.set(f.path, '?');
  }
  gitFileStatuses.set(map);
}

// =============================================================================
// Auto-refresh
// =============================================================================

let refreshInterval: ReturnType<typeof setInterval> | null = null;

export function startGitRefresh(instanceId: string): void {
  stopGitRefresh();
  refreshInterval = setInterval(() => {
    const tab = get(gitTab);
    const open = get(isGitOpen);
    if (!open) return;
    if (tab === 'log') {
      const branchFilter = get(logBranchFilter);
      fetchGitLog(instanceId, branchFilter ? { branch: branchFilter } : undefined);
    } else if (tab === 'branches') fetchGitBranches(instanceId);
    else if (tab === 'status') fetchGitStatus(instanceId);
  }, 8000);
}

export function stopGitRefresh(): void {
  if (refreshInterval) {
    clearInterval(refreshInterval);
    refreshInterval = null;
  }
}
