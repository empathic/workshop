/**
 * Unified File Store
 *
 * Combines file explorer (directory browsing) and file viewer (content display)
 * into a single store. Both concern the same domain: files within an instance's
 * working directory.
 *
 * Formerly: fileExplorer.ts + fileViewer.ts
 */

import { writable, derived, get } from 'svelte/store';
import { currentInstance } from './instances';
import { updateUrl } from '$lib/utils/url';
import { apiGet } from '$lib/utils/api';
import { isGitOpen } from './git';
import type { GitDiffFile } from './git';

// =============================================================================
// Explorer Types
// =============================================================================

export interface FileEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  isSymlink?: boolean;
  symlinkTarget?: string;
  size?: number;
  modifiedAt?: string;
}

export interface DirectoryListing {
  path: string;
  entries: FileEntry[];
  error?: string;
}

export interface FileExplorerState {
  isOpen: boolean;
  currentPath: string;
  listings: Map<string, DirectoryListing>;
  loading: Set<string>;
  expanded: Set<string>;
  selectedPath: string | null;
}

// =============================================================================
// Viewer Types
// =============================================================================

export interface DiffContext {
  commit?: string;
  base?: string;
  head?: string;
  diffMode?: 'twodot' | 'threedot';
}

export interface FileViewerState {
  isOpen: boolean;
  filePath: string | null;
  content: string | null;
  language: string | null;
  lineNumber: number | null;
  /** Source of the file content - 'tool' from tool output, 'fetch' from API */
  source: 'tool' | 'fetch' | null;
  /** Diff data for the current file, if available */
  diffData: GitDiffFile | null;
  /** View mode — 'content' for normal file view, 'diff' for diff view */
  viewMode: 'content' | 'diff';
  /** Whether a diff is currently being fetched */
  diffLoading: boolean;
  /** Error message if diff fetch failed */
  diffError: string | null;
  /** Context for multi-file diff navigation */
  diffContext: DiffContext | null;
  /** All files in the current diff (for multi-file navigation) */
  allDiffFiles: GitDiffFile[];
  /** Index of the currently displayed file within allDiffFiles */
  currentFileIndex: number;
}

// =============================================================================
// Explorer Store
// =============================================================================

const explorerInitialState: FileExplorerState = {
  isOpen: false,
  currentPath: '',
  listings: new Map(),
  loading: new Set(),
  expanded: new Set(),
  selectedPath: null
};

export const fileExplorerState = writable<FileExplorerState>(explorerInitialState);

/** Pending search query to pre-seed the file browser's search input */
export const pendingSearchQuery = writable<string>('');

/**
 * When non-null, the file explorer is in "picker" mode.
 * File selection invokes `onSelect` instead of opening the global file viewer.
 * The `label` is shown in the explorer header so the user knows why the panel opened.
 */
export interface FilePickerRequest {
  onSelect: (path: string) => void;
  label: string;
}
export const filePickerRequest = writable<FilePickerRequest | null>(null);

// =============================================================================
// Explorer Derived Stores
// =============================================================================

export const isExplorerOpen = derived(fileExplorerState, ($state) => $state.isOpen);
export const currentExplorerPath = derived(fileExplorerState, ($state) => $state.currentPath);
export const selectedFilePath = derived(fileExplorerState, ($state) => $state.selectedPath);

/** Get the root directory (instance's working_dir) */
export const rootDirectory = derived(currentInstance, ($instance) => $instance?.working_dir ?? '/');

/** Get the listing for the current path */
export const currentListing = derived([fileExplorerState, currentExplorerPath], ([$state, $path]) =>
  $state.listings.get($path)
);

/** Check if a path is currently loading */
export function isLoading(path: string): boolean {
  return get(fileExplorerState).loading.has(path);
}

/** Check if a directory is expanded */
export function isExpanded(path: string): boolean {
  return get(fileExplorerState).expanded.has(path);
}

// =============================================================================
// Explorer Actions
// =============================================================================

/** Open the file explorer */
export function openExplorer(): void {
  const instance = get(currentInstance);
  if (!instance) return;

  fileExplorerState.update((state) => ({
    ...state,
    isOpen: true,
    currentPath: instance.working_dir
  }));

  // Load the root directory
  loadDirectory(instance.working_dir);
  updateUrl({ explorer: get(isGitOpen) ? 'git' : 'files' });
}

/**
 * Open the file explorer in picker mode.
 * When the user selects a file, `onSelect` is called with the absolute path
 * and the explorer closes. The caller owns the action (e.g. setPaneContent).
 */
export function openExplorerPicker(onSelect: (path: string) => void, label = 'Select a file'): void {
  filePickerRequest.set({ onSelect, label });
  isGitOpen.set(false);
  openExplorer();
}

/** Close the file explorer */
export function closeExplorer(): void {
  fileExplorerState.update((state) => ({
    ...state,
    isOpen: false
  }));
  filePickerRequest.set(null);
  updateUrl({ explorer: null });
}

/** Toggle the file explorer */
export function toggleExplorer(): void {
  const state = get(fileExplorerState);
  if (state.isOpen) {
    closeExplorer();
  } else {
    openExplorer();
  }
}

/** Navigate to a directory */
export function navigateToDirectory(path: string): void {
  fileExplorerState.update((state) => ({
    ...state,
    currentPath: path
  }));
  loadDirectory(path);
}

/** Toggle expansion of a directory in tree view */
export function toggleDirectory(path: string): void {
  fileExplorerState.update((state) => {
    const expanded = new Set(state.expanded);
    if (expanded.has(path)) {
      expanded.delete(path);
    } else {
      expanded.add(path);
      // Load the directory if not already loaded
      if (!state.listings.has(path)) {
        loadDirectory(path);
      }
    }
    return { ...state, expanded };
  });
}

/** Select a file */
export function selectFile(path: string): void {
  fileExplorerState.update((state) => ({
    ...state,
    selectedPath: path
  }));
}

/** Load a directory listing from the API */
export async function loadDirectory(path: string): Promise<void> {
  const instance = get(currentInstance);
  if (!instance) return;

  // Mark as loading
  fileExplorerState.update((state) => {
    const loading = new Set(state.loading);
    loading.add(path);
    return { ...state, loading };
  });

  try {
    const listing = await apiGet<DirectoryListing>(
      `/api/instances/${instance.id}/files?path=${encodeURIComponent(path)}`
    );

    fileExplorerState.update((state) => {
      const listings = new Map(state.listings);
      const loading = new Set(state.loading);
      listings.set(path, listing);
      loading.delete(path);
      return { ...state, listings, loading };
    });
  } catch (error) {
    console.error('Failed to load directory:', error);

    // Check if it's a 404 - API not implemented yet
    const errorMsg = error instanceof Error ? error.message : 'Failed to load directory';
    const isNotImplemented = errorMsg.includes('404');

    fileExplorerState.update((state) => {
      const listings = new Map(state.listings);
      const loading = new Set(state.loading);
      listings.set(path, {
        path,
        entries: [],
        error: isNotImplemented
          ? 'File browser API not yet implemented. Backend needs: GET /api/instances/{id}/files?path=...'
          : errorMsg
      });
      loading.delete(path);
      return { ...state, listings, loading };
    });
  }
}

/** Fetch file content from the API */
export async function fetchFileContent(path: string): Promise<string> {
  const instance = get(currentInstance);
  if (!instance) throw new Error('No instance selected');

  const response = await apiGet<{ content: string }>(
    `/api/instances/${instance.id}/files/content?path=${encodeURIComponent(path)}`
  );
  return response.content;
}

/** Go up one directory */
export function navigateUp(): void {
  const state = get(fileExplorerState);
  const root = get(rootDirectory);

  if (state.currentPath === root) return;

  const parts = state.currentPath.split('/').filter(Boolean);
  parts.pop();
  const parentPath = '/' + parts.join('/');

  navigateToDirectory(parentPath || root);
}

/** Open the file explorer with a search query pre-seeded (for fuzzy find fallback). */
export function openExplorerWithSearch(query: string): void {
  const instance = get(currentInstance);
  if (!instance) return;

  // Ensure we're on the Files tab, not Git
  isGitOpen.set(false);

  fileExplorerState.update((state) => ({
    ...state,
    isOpen: true,
    currentPath: instance.working_dir
  }));

  // Load the root directory so the browser is ready
  loadDirectory(instance.working_dir);

  // Set the pending search query — FileBrowser will pick this up
  pendingSearchQuery.set(query);
  updateUrl({ explorer: 'files' });
}

/** Navigate the explorer to the directory containing a file path. */
export function navigateExplorerToFile(filePath: string): void {
  const instance = get(currentInstance);
  if (!instance) return;

  // Extract the parent directory from the file path
  const parts = filePath.split('/');
  parts.pop(); // remove filename
  const parentDir = parts.join('/') || instance.working_dir;

  // Resolve relative to working_dir if not absolute
  const targetDir = parentDir.startsWith('/') ? parentDir : instance.working_dir + '/' + parentDir;

  fileExplorerState.update((state) => ({
    ...state,
    currentPath: targetDir
  }));

  loadDirectory(targetDir);
}

/** Reset explorer state (e.g., when switching instances) */
export function resetExplorer(): void {
  fileExplorerState.set(explorerInitialState);
  filePickerRequest.set(null);
}

// =============================================================================
// Explorer Helpers
// =============================================================================

/** Get icon for a file based on extension */
export function getFileIcon(entry: FileEntry): string {
  // Symlinks get a special indicator
  if (entry.isSymlink) {
    if (entry.isDirectory) return '📁↗';
    return '🔗';
  }

  if (entry.isDirectory) return '📁';

  const ext = entry.name.split('.').pop()?.toLowerCase() ?? '';

  const icons: Record<string, string> = {
    // Code
    ts: '📘',
    tsx: '📘',
    js: '📒',
    jsx: '📒',
    svelte: '🧡',
    vue: '💚',
    rs: '🦀',
    go: '🐹',
    py: '🐍',
    rb: '💎',
    java: '☕',
    // Config
    json: '📋',
    yaml: '📋',
    yml: '📋',
    toml: '📋',
    // Docs
    md: '📝',
    txt: '📄',
    // Style
    css: '🎨',
    scss: '🎨',
    // Data
    sql: '🗃️',
    // Shell
    sh: '🖥️',
    bash: '🖥️',
    zsh: '🖥️'
  };

  return icons[ext] ?? '📄';
}

/** Format file size */
export function formatFileSize(bytes?: number | null): string {
  if (bytes === undefined || bytes === null) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// =============================================================================
// Viewer Store
// =============================================================================

const viewerInitialState: FileViewerState = {
  isOpen: false,
  filePath: null,
  content: null,
  language: null,
  lineNumber: null,
  source: null,
  diffData: null,
  viewMode: 'content',
  diffLoading: false,
  diffError: null,
  diffContext: null,
  allDiffFiles: [],
  currentFileIndex: 0
};

export const fileViewerState = writable<FileViewerState>(viewerInitialState);

// =============================================================================
// Viewer Derived Stores
// =============================================================================

export const isFileViewerOpen = derived(fileViewerState, ($state) => $state.isOpen);
export const currentFilePath = derived(fileViewerState, ($state) => $state.filePath);
export const currentFileContent = derived(fileViewerState, ($state) => $state.content);
export const currentFileLanguage = derived(fileViewerState, ($state) => $state.language);
export const currentLineNumber = derived(fileViewerState, ($state) => $state.lineNumber);
export const currentDiffData = derived(fileViewerState, ($state) => $state.diffData);
export const currentViewMode = derived(fileViewerState, ($state) => $state.viewMode);
export const isDiffLoading = derived(fileViewerState, ($state) => $state.diffLoading);
export const diffError = derived(fileViewerState, ($state) => $state.diffError);
export const currentDiffContext = derived(fileViewerState, ($state) => $state.diffContext);
export const allDiffFiles = derived(fileViewerState, ($state) => $state.allDiffFiles);
export const currentFileIndex = derived(fileViewerState, ($state) => $state.currentFileIndex);

// =============================================================================
// Language Detection
// =============================================================================

const EXTENSION_MAP: Record<string, string> = {
  // JavaScript/TypeScript
  js: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  tsx: 'typescript',
  mjs: 'javascript',
  cjs: 'javascript',

  // Web
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'scss',
  sass: 'sass',
  less: 'less',
  svelte: 'html',
  vue: 'html',

  // Systems
  rs: 'rust',
  go: 'go',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',

  // Scripting
  py: 'python',
  rb: 'ruby',
  php: 'php',
  pl: 'perl',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  fish: 'bash',

  // Data
  json: 'json',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  xml: 'xml',
  csv: 'plaintext',

  // Config
  md: 'markdown',
  markdown: 'markdown',
  txt: 'plaintext',
  log: 'plaintext',
  env: 'bash',
  gitignore: 'plaintext',
  dockerignore: 'plaintext',
  editorconfig: 'ini',

  // Java/JVM
  java: 'java',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  groovy: 'groovy',

  // Other
  sql: 'sql',
  graphql: 'graphql',
  gql: 'graphql',
  proto: 'protobuf',
  swift: 'swift',
  dart: 'dart',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hrl: 'erlang',
  lua: 'lua',
  vim: 'vim',
  zig: 'zig',
  nim: 'nim',
  r: 'r',
  R: 'r',
  jl: 'julia',
  clj: 'clojure',
  cljs: 'clojure',
  hs: 'haskell',
  elm: 'elm',
  ml: 'ocaml',
  mli: 'ocaml',
  fs: 'fsharp',
  fsx: 'fsharp'
};

const FILENAME_MAP: Record<string, string> = {
  Makefile: 'makefile',
  makefile: 'makefile',
  GNUmakefile: 'makefile',
  Dockerfile: 'dockerfile',
  Containerfile: 'dockerfile',
  'docker-compose.yml': 'yaml',
  'docker-compose.yaml': 'yaml',
  'package.json': 'json',
  'tsconfig.json': 'json',
  'Cargo.toml': 'toml',
  'Cargo.lock': 'toml',
  'go.mod': 'go',
  'go.sum': 'plaintext',
  'requirements.txt': 'plaintext',
  Gemfile: 'ruby',
  Rakefile: 'ruby',
  'CMakeLists.txt': 'cmake'
};

function detectLanguage(filePath: string): string {
  const filename = filePath.split('/').pop() ?? '';

  // Check exact filename matches first
  if (FILENAME_MAP[filename]) {
    return FILENAME_MAP[filename];
  }

  // Check extension
  const ext = filename.split('.').pop()?.toLowerCase() ?? '';
  return EXTENSION_MAP[ext] ?? 'plaintext';
}

// =============================================================================
// Viewer Actions
// =============================================================================

/**
 * Open the file viewer with content from a tool result.
 * This is the primary way to populate the viewer.
 */
export function openFileFromTool(filePath: string, content: string, lineNumber?: number): void {
  fileViewerState.set({
    isOpen: true,
    filePath,
    content,
    language: detectLanguage(filePath),
    lineNumber: lineNumber ?? null,
    source: 'tool',
    diffData: null,
    viewMode: 'content',
    diffLoading: false,
    diffError: null,
    diffContext: null,
    allDiffFiles: [],
    currentFileIndex: 0
  });
  updateUrl({ file: filePath, line: lineNumber ? String(lineNumber) : null, view: null, commit: null });
}

/**
 * Open the file viewer for a path — fetches content from the API.
 * On success, opens the viewer and syncs the explorer to the parent directory.
 * On 404, falls back to opening the file explorer with the path as a search query.
 */
export function openFilePath(filePath: string, lineNumber?: number): void {
  // Show the viewer immediately with a loading state
  fileViewerState.set({
    isOpen: true,
    filePath,
    content: null,
    language: detectLanguage(filePath),
    lineNumber: lineNumber ?? null,
    source: 'fetch',
    diffData: null,
    viewMode: 'content',
    diffLoading: false,
    diffError: null,
    diffContext: null,
    allDiffFiles: [],
    currentFileIndex: 0
  });
  updateUrl({ file: filePath, line: lineNumber ? String(lineNumber) : null, view: null, commit: null });

  // Actually fetch the content
  fetchFileContent(filePath)
    .then((content) => {
      // Verify we're still looking at the same file
      const current = get(fileViewerState);
      if (current.filePath !== filePath) return;

      setFileContent(content);
      // Sync the explorer to this file's directory
      navigateExplorerToFile(filePath);
    })
    .catch(() => {
      // File not found — close the empty viewer and open explorer with search
      const current = get(fileViewerState);
      if (current.filePath !== filePath) return;

      closeFileViewer();
      // Convert partial path to a glob so the search is exact, not fuzzy.
      // "utils.ts" → "**/utils.ts", "lib/utils.ts" → "**/lib/utils.ts"
      const globQuery = filePath.startsWith('/') ? filePath : '**/' + filePath;
      openExplorerWithSearch(globQuery);
    });
}

/** Update the content for the current file (e.g., after fetching from API). */
export function setFileContent(content: string): void {
  fileViewerState.update((state) => ({
    ...state,
    content
  }));
}

/** Navigate to a specific line in the current file. */
export function goToLine(lineNumber: number): void {
  fileViewerState.update((state) => ({
    ...state,
    lineNumber
  }));
  updateUrl({ line: String(lineNumber) });
}

/** Open the file viewer in diff mode. */
export function openFileDiff(filePath: string, diffFile: GitDiffFile, commit?: string): void {
  fileViewerState.set({
    isOpen: true,
    filePath,
    content: null,
    language: detectLanguage(filePath),
    lineNumber: null,
    source: null,
    diffData: diffFile,
    viewMode: 'diff',
    diffLoading: false,
    diffError: null,
    diffContext: null,
    allDiffFiles: [],
    currentFileIndex: 0
  });
  updateUrl({ file: filePath, line: null, view: 'diff', commit: commit ?? null });
}

/** Open the file viewer in diff-loading state (drawer opens immediately with spinner). */
export function openFileDiffLoading(filePath: string, commit?: string): void {
  fileViewerState.set({
    isOpen: true,
    filePath,
    content: null,
    language: detectLanguage(filePath),
    lineNumber: null,
    source: null,
    diffData: null,
    viewMode: 'diff',
    diffLoading: true,
    diffError: null,
    diffContext: null,
    allDiffFiles: [],
    currentFileIndex: 0
  });
  updateUrl({ file: filePath, line: null, view: 'diff', commit: commit ?? null });
}

/**
 * Open the file viewer in diff mode with full multi-file diff context.
 * The viewer will fetch the full diff and enable file navigation.
 */
export function openFileDiffWithContext(filePath: string, context: DiffContext): void {
  fileViewerState.set({
    isOpen: true,
    filePath,
    content: null,
    language: detectLanguage(filePath),
    lineNumber: null,
    source: null,
    diffData: null,
    viewMode: 'diff',
    diffLoading: true,
    diffError: null,
    diffContext: context,
    allDiffFiles: [],
    currentFileIndex: 0
  });
  updateUrl({ file: filePath, line: null, view: 'diff', commit: context.commit ?? null });
}

/**
 * Navigate to a different file within a multi-file diff.
 * Does NOT re-trigger drawer animation or URL updates.
 */
export function navigateDiffFile(index: number): void {
  fileViewerState.update((state) => {
    if (index < 0 || index >= state.allDiffFiles.length) return state;
    const file = state.allDiffFiles[index];
    return {
      ...state,
      filePath: file.path,
      language: detectLanguage(file.path),
      diffData: file,
      currentFileIndex: index,
      diffError: null
    };
  });
}

/** Set all diff files for multi-file navigation. */
export function setAllDiffFiles(files: GitDiffFile[], initialIndex: number): void {
  fileViewerState.update((state) => ({
    ...state,
    allDiffFiles: files,
    currentFileIndex: initialIndex,
    diffData: files[initialIndex] ?? state.diffData,
    diffLoading: false,
    diffError: null
  }));
}

/** Resolve a pending diff load with data. */
export function setDiffData(diffFile: GitDiffFile): void {
  fileViewerState.update((state) => ({
    ...state,
    diffData: diffFile,
    diffLoading: false,
    diffError: null
  }));
}

/** Mark a diff load as failed. */
export function setDiffError(message?: string): void {
  fileViewerState.update((state) => ({
    ...state,
    diffLoading: false,
    diffError: message ?? 'Failed to load diff'
  }));
}

/** Toggle between content and diff view modes. */
export function toggleViewMode(): void {
  fileViewerState.update((state) => {
    const newMode = state.viewMode === 'content' ? 'diff' : 'content';
    updateUrl({ view: newMode === 'diff' ? 'diff' : null, commit: null });
    return { ...state, viewMode: newMode };
  });
}

/** Close the file viewer. */
export function closeFileViewer(): void {
  fileViewerState.set(viewerInitialState);
  updateUrl({ file: null, line: null, view: null, commit: null });
}

/** Toggle the file viewer open/closed. */
export function toggleFileViewer(): void {
  fileViewerState.update((state) => ({
    ...state,
    isOpen: !state.isOpen
  }));
}
