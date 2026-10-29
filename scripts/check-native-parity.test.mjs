import assert from 'node:assert/strict';
import { after, describe, it } from 'node:test';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { scan } from './check-native-parity.mjs';

const roots = [];

function fixture({ deps = {}, files = {} } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'native-parity-'));
  roots.push(root);
  const client = join(root, 'packages/client');
  for (const dir of ['src', 'functions', 'public', 'installer', 'e2e']) mkdirSync(join(client, dir), { recursive: true });
  writeFileSync(join(client, 'vite.config.ts'), 'export default {};\n');
  writeFileSync(join(client, 'package.json'), JSON.stringify({ name: 'fixture', dependencies: deps }));
  writeFileSync(
    join(client, 'src/local.ts'),
    "import { local } from './local';\n// import { Capacitor } from '@capacitor/core';\nconst text = \"import 'electron'\";\n",
  );
  writeFileSync(join(client, 'public/sw.js'), 'self.addEventListener("install", () => {});\n');
  writeFileSync(join(client, 'installer/bake.mjs'), 'export const value = 1;\n');
  writeFileSync(join(client, 'e2e/run.mjs'), 'export const value = 2;\n');
  writeFileSync(join(client, 'functions/api.ts'), 'export const value = 3;\n');
  for (const [path, code] of Object.entries(files)) {
    const full = join(client, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, code);
  }
  return { root, client };
}

after(() => {
  for (const root of roots) rmSync(root, { recursive: true, force: true });
});

describe('check-native-parity', () => {
  it('passes the real client tree', () => {
    const { issues, missing } = scan();
    assert.deepEqual(missing, []);
    assert.deepEqual(issues, []);
  });

  it('ignores native names in comments and strings', () => {
    const { root } = fixture();
    const { issues, missing, scanned } = scan({ root });
    assert.deepEqual(missing, []);
    assert.deepEqual(issues, []);
    assert.ok(scanned >= 6, `expected at least 6 files, got ${scanned}`);
  });

  it('fails on a static native import', () => {
    const { root } = fixture({ files: { 'src/bad.ts': "import { Capacitor } from '@capacitor/core';\n" } });
    const { issues } = scan({ root });
    assert.equal(issues.length, 1);
    assert.match(issues[0], /@capacitor\/core/);
  });

  it('fails on a type-only native import', () => {
    const { root } = fixture({ files: { 'src/bad.ts': "import type { CapacitorConfig } from '@capacitor/cli';\n" } });
    assert.match(scan({ root }).issues[0], /@capacitor\/cli/);
  });

  it('fails on a community plugin import', () => {
    const { root } = fixture({ files: { 'src/bad.ts': "import '@capacitor-community/keep-awake';\n" } });
    assert.match(scan({ root }).issues[0], /@capacitor-community\/keep-awake/);
  });

  it('fails on require, dynamic import and import-equals', () => {
    const { root } = fixture({
      files: {
        'src/require.cjs': "const remote = require('@electron/remote');\n",
        'src/dynamic.ts': "export const load = () => import('electron');\n",
        'src/equals.ts': "import app = require('@capacitor/app');\n",
      },
    });
    const { issues } = scan({ root });
    assert.equal(issues.length, 3);
    assert.ok(issues.some((issue) => issue.includes('@electron/remote')));
    assert.ok(issues.some((issue) => issue.includes('electron')));
    assert.ok(issues.some((issue) => issue.includes('@capacitor/app')));
  });

  it('fails on an import inside a Svelte script block', () => {
    const { root } = fixture({
      files: { 'src/Bad.svelte': "<script lang=\"ts\">import { App } from '@capacitor/app';</script>\n<p>hi</p>\n" },
    });
    assert.match(scan({ root }).issues[0], /@capacitor\/app/);
  });

  it('fails on export-from, an electron subpath and require.resolve', () => {
    const { root } = fixture({
      files: {
        'src/export.ts': "export { app } from '@capacitor/app';\n",
        'src/subpath.ts': "import 'electron/main';\n",
        'src/resolve.cjs': "const entry = require.resolve('@electron/remote/main');\n",
      },
    });
    const { issues } = scan({ root });
    assert.equal(issues.length, 3);
    assert.ok(issues.some((issue) => issue.includes('@capacitor/app')));
    assert.ok(issues.some((issue) => issue.includes('electron/main')));
    assert.ok(issues.some((issue) => issue.includes('@electron/remote')));
  });

  it('fails on a native dependency in package.json', () => {
    const { root } = fixture({ deps: { '@capacitor/core': '8.5.3', electron: '45.0.0' } });
    const { issues } = scan({ root });
    assert.equal(issues.length, 2);
    assert.ok(issues.every((issue) => issue.startsWith('packages/client/package.json:')));
  });

  it('does not over-match packages that merely start with electron', () => {
    const { root } = fixture({ files: { 'src/ok.ts': "import isDev from 'electron-is-dev';\n" } });
    assert.deepEqual(scan({ root }).issues, []);
  });

  it('fails when a covered path is missing', () => {
    const { root, client } = fixture();
    rmSync(join(client, 'public'), { recursive: true, force: true });
    const { missing } = scan({ root });
    assert.deepEqual(missing, ['public']);
  });
});
