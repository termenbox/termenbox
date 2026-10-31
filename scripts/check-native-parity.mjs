#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { extname, join, relative, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Guardrail `native-wrapper-keeps-web-parity`: native code lives in packages/native and the web
// client must never import or depend on it. This gate parses the client's module graph instead of
// grepping, so a native name in a comment or a string does not fail the build.

const REPO_ROOT = fileURLToPath(new URL('..', import.meta.url));
const CLIENT = 'packages/client';
const COVERED = ['src', 'functions', 'public', 'installer', 'e2e', 'vite.config.ts', 'package.json'];
const SCANNED_DIRS = ['src', 'functions', 'public', 'installer', 'e2e'];
const SOURCE_EXT = new Set(['.ts', '.tsx', '.js', '.mjs', '.cjs', '.svelte']);
const SKIP_DIRS = new Set(['node_modules', 'dist', 'build', 'coverage', '.svelte-kit', '.git']);
const DEP_FIELDS = ['dependencies', 'devDependencies', 'peerDependencies', 'optionalDependencies'];

/** True for the packages a wrapper may use but the web client may not. */
export function isNativeSpecifier(specifier) {
  return (
    specifier === 'electron' ||
    specifier.startsWith('electron/') ||
    specifier.startsWith('@capacitor/') ||
    specifier.startsWith('@capacitor-community/') ||
    specifier.startsWith('@electron/')
  );
}

function loadTypeScript() {
  const require = createRequire(join(REPO_ROOT, CLIENT, 'package.json'));
  try {
    return require('typescript');
  } catch {
    throw new Error('typescript is not installed; run `pnpm install` before the gate');
  }
}

function walk(dir) {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name)) files.push(...walk(join(dir, entry.name)));
    } else if (SOURCE_EXT.has(extname(entry.name))) {
      files.push(join(dir, entry.name));
    }
  }
  return files;
}

/** The contents of every `<script>` block; Svelte markup outside them carries no imports. */
function svelteScripts(text) {
  return [...text.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)].map((match) => match[1]).join('\n');
}

function scriptKind(ts, file) {
  if (file.endsWith('.tsx')) return ts.ScriptKind.TSX;
  if (file.endsWith('.js') || file.endsWith('.mjs') || file.endsWith('.cjs')) return ts.ScriptKind.JS;
  return ts.ScriptKind.TS;
}

/** Module specifiers of `import`, `export ... from`, `import()`, `require()` and `import = require()`. */
function moduleSpecifiers(ts, file, code) {
  const source = ts.createSourceFile(file, code, ts.ScriptTarget.Latest, true, scriptKind(ts, file));
  const specifiers = new Set();
  const add = (node) => {
    if (node && ts.isStringLiteralLike(node)) specifiers.add(node.text);
  };
  const visit = (node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      add(node.moduleSpecifier);
    } else if (ts.isImportEqualsDeclaration(node) && ts.isExternalModuleReference(node.moduleReference)) {
      add(node.moduleReference.expression);
    } else if (ts.isCallExpression(node)) {
      if (node.expression.kind === ts.SyntaxKind.ImportKeyword) add(node.arguments[0]);
      else if (ts.isIdentifier(node.expression) && node.expression.text === 'require') add(node.arguments[0]);
      else if (
        ts.isPropertyAccessExpression(node.expression) &&
        ts.isIdentifier(node.expression.expression) &&
        node.expression.expression.text === 'require'
      ) {
        add(node.arguments[0]);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return [...specifiers];
}

/**
 * Scans one tree. `root` exists for the self-test; the real run uses the repository root.
 * Returns `{ issues, missing, scanned }`; `missing` makes a covered path that vanished fail rather
 * than silently passing.
 */
export function scan({ root = REPO_ROOT, ts } = {}) {
  const client = join(root, CLIENT);
  const missing = COVERED.filter((entry) => !existsSync(join(client, entry)));
  const issues = [];
  let scanned = 0;
  if (missing.length > 0) return { issues, missing, scanned };
  ts ??= loadTypeScript();

  const manifest = join(client, 'package.json');
  try {
    const pkg = JSON.parse(readFileSync(manifest, 'utf8'));
    for (const field of DEP_FIELDS) {
      for (const name of Object.keys(pkg[field] ?? {})) {
        if (isNativeSpecifier(name)) issues.push(`${CLIENT}/package.json: ${field}.${name}`);
      }
    }
  } catch (error) {
    issues.push(`${CLIENT}/package.json: cannot parse (${error.message})`);
  }

  for (const entry of SCANNED_DIRS) {
    for (const file of walk(join(client, entry))) {
      const rel = relative(root, file).split(sep).join('/');
      const code = file.endsWith('.svelte') ? svelteScripts(readFileSync(file, 'utf8')) : readFileSync(file, 'utf8');
      if (!code.trim()) continue;
      scanned += 1;
      for (const specifier of moduleSpecifiers(ts, rel, code)) {
        if (isNativeSpecifier(specifier)) issues.push(`${rel}: imports ${specifier}`);
      }
    }
  }

  const vite = `${CLIENT}/vite.config.ts`;
  scanned += 1;
  for (const specifier of moduleSpecifiers(ts, vite, readFileSync(join(root, vite), 'utf8'))) {
    if (isNativeSpecifier(specifier)) issues.push(`${vite}: imports ${specifier}`);
  }

  return { issues, missing, scanned };
}

function main() {
  let result;
  try {
    result = scan();
  } catch (error) {
    console.error(`native parity: ${error.message}`);
    process.exit(1);
  }
  if (result.missing.length > 0) {
    console.error('native parity: covered paths are missing:');
    for (const entry of result.missing) console.error(`  ${CLIENT}/${entry}`);
    process.exit(1);
  }
  if (result.issues.length > 0) {
    console.error('native parity: packages/client must not import or depend on native packages:');
    for (const issue of result.issues) console.error(`  ${issue}`);
    process.exit(1);
  }
  console.log(`native parity: ok (${result.scanned} files scanned)`);
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) main();
