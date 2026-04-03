/**
 * Pure file path extraction and matching logic.
 *
 * Separated from fileLinks.ts so it can be tested without store dependencies.
 */

/**
 * Regex to match file paths in text.
 * Matches:
 * - Absolute paths: /foo/bar/baz.ts
 * - Relative paths: src/foo/bar.ts, ./foo/bar.ts
 * - With optional line number: :123
 */
export const FILE_PATH_REGEX = /(?:^|[\s\(\"\'`])(((?:\/|\.\/|\.\.\/)?(?:[\w.-]+\/)+[\w.-]+\.[\w]+)(?::(\d+))?)/g;

/**
 * Common file extensions we want to linkify
 */
export const FILE_EXTENSIONS = new Set([
  'ts',
  'tsx',
  'js',
  'jsx',
  'mjs',
  'cjs',
  'svelte',
  'vue',
  'html',
  'htm',
  'css',
  'scss',
  'sass',
  'less',
  'json',
  'yaml',
  'yml',
  'toml',
  'xml',
  'rs',
  'go',
  'py',
  'rb',
  'php',
  'java',
  'kt',
  'scala',
  'c',
  'cpp',
  'h',
  'hpp',
  'cc',
  'cxx',
  'sh',
  'bash',
  'zsh',
  'fish',
  'md',
  'markdown',
  'txt',
  'log',
  'sql',
  'graphql',
  'gql',
  'proto',
  'swift',
  'dart',
  'ex',
  'exs',
  'erl',
  'lua',
  'vim',
  'zig',
  'nim',
  'r',
  'R',
  'jl',
  'clj',
  'cljs',
  'hs',
  'elm',
  'ml',
  'fs'
]);

/**
 * Check if a path has a valid file extension
 */
export function hasValidExtension(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return FILE_EXTENSIONS.has(ext);
}

/**
 * Extract file paths from text content
 */
export function extractFilePaths(text: string): Array<{ path: string; line?: number; start: number; end: number }> {
  const paths: Array<{ path: string; line?: number; start: number; end: number }> = [];
  let match: RegExpExecArray | null;

  // Reset regex
  FILE_PATH_REGEX.lastIndex = 0;

  while ((match = FILE_PATH_REGEX.exec(text)) !== null) {
    const fullMatch = match[1]!;
    const filePath = match[2]!;
    const lineNum = match[3] ? parseInt(match[3], 10) : undefined;

    if (hasValidExtension(filePath)) {
      // Adjust start to account for leading whitespace/delimiter
      const start = match.index + (match[0].length - fullMatch.length);
      const entry: { path: string; line?: number; start: number; end: number } = {
        path: filePath,
        start,
        end: start + fullMatch.length
      };
      if (lineNum !== undefined) entry.line = lineNum;
      paths.push(entry);
    }
  }

  return paths;
}
