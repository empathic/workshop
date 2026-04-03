const path = require('path');
const fs = require('fs');

// Jest V8 coverage + Bazel: a path namespace mismatch.
//
// Jest's rootDir defaults to the config file's directory (the runfiles tree).
// V8's profiler resolves symlinks via libuv (uv_fs_realpath) and records URLs
// against the *real* execroot. Jest's coverage filter does:
//
//   res.url.startsWith(this._config.rootDir)
//
// Runfiles path != real execroot path, so every entry is silently dropped → 0%.
//
// The fix has three parts:
//
// 1. rootDir → real execroot bin dir (so the startsWith check passes)
// 2. roots → [runfiles tree, real bin dir] (rootDir controls coverage filtering,
//    but Jest also uses it for test discovery; roots restores that)
// 3. reporters → absolute paths (rootDir change breaks relative resolution)
//
// The sandbox complicates this: Bazel's darwin-sandbox creates real directories
// but populates them with symlinks to the real execroot. realpathSync on the
// *directory* is a no-op (it's already real). We have to realpathSync a *file*
// inside the runfiles tree (which IS a symlink) and extract the real execroot
// prefix from the resolved path. See docs/postmortem/jest_coverage_bazel.md.
let bazelBinDir;
let bazelRoots;
if (process.env.JS_BINARY__EXECROOT && process.env.JS_BINARY__BINDIR && process.env.JS_BINARY__PACKAGE) {
  const pkg = process.env.JS_BINARY__PACKAGE;
  const binSuffix = path.join(process.env.JS_BINARY__BINDIR, pkg);
  const rawBinDir = path.resolve(process.env.JS_BINARY__EXECROOT, binSuffix);
  const runfilesBase = process.env.RUNFILES;
  const workspace = process.env.JS_BINARY__WORKSPACE;

  let realBinDir = rawBinDir;
  if (runfilesBase && workspace) {
    try {
      const probeFile = path.join(runfilesBase, workspace, pkg, 'jest.config.cjs');
      const realProbe = fs.realpathSync(probeFile);
      const idx = realProbe.indexOf(binSuffix);
      if (idx !== -1) {
        realBinDir = realProbe.substring(0, idx + binSuffix.length);
      }
    } catch {
      // Outside sandbox — rawBinDir is already correct.
    }
    bazelRoots = [path.join(runfilesBase, workspace, pkg), realBinDir];
  }
  bazelBinDir = realBinDir;
}

let jestJunitPath;
try {
  jestJunitPath = require.resolve('jest-junit');
} catch {
  /* not available outside Bazel */
}

/** @type {import('jest').Config} */
module.exports = {
  testEnvironment: 'node',
  testMatch: ['**/dist-test/**/*.test.js'],
  transform: {},
  coverageProvider: 'v8',
  moduleFileExtensions: ['js', 'mjs', 'json'],
  snapshotFormat: { escapeString: true, printBasicPrototype: true },
  collectCoverageFrom: ['**/dist-test/**/*.js', '!**/*.test.js'],
  coveragePathIgnorePatterns: ['/node_modules/', '\\.runfiles/'],
  coverageThreshold: {
    global: { branches: 80, functions: 80, lines: 80, statements: 80 }
  },
  clearMocks: true,
  verbose: true,
  injectGlobals: true,
  ...(bazelBinDir
    ? {
        rootDir: bazelBinDir,
        ...(bazelRoots ? { roots: bazelRoots } : {}),
        reporters: [
          'default',
          ...(jestJunitPath ? [[jestJunitPath, { outputFile: process.env.XML_OUTPUT_FILE || 'jest-junit.xml' }]] : [])
        ]
      }
    : {})
};
