// The four New workspace dialog walkthroughs (DECISION-0148). Each is a function of `c` (see render.mjs) and ends with an
// assertion of its final state, so a walkthrough that did not reach it fails instead of producing a video of something else
// (GUARDRAIL ux-scenarios-assert-a-state-before-they-capture-it). They run on the scripted server and its fake home.
import { HOME } from '../fake-tree.mjs';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { noteAssertion } from './lib.mjs';

const expectThat = (cond, what) => { if (!cond) throw new Error(`final state: ${what}`); noteAssertion(); };

/** Opens the dialog the way the profile's user does: the visible New button, else the command palette. */
async function openDialog({ h, page, dialog, q }) {
  const plus = page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first();
  await h.say('Start a new workspace');
  if (await plus.isVisible()) await h.tap(plus);
  else {
    await page.keyboard.press('Control+K');
    await h.pause();
    await h.tap(page.getByRole('option', { name: /New workspace/ }).first());
  }
  await dialog.waitFor({ state: 'visible', timeout: 5000 });
  await q('agent-row').waitFor({ state: 'visible', timeout: 5000 });
  expectThat(await dialog.isVisible(), 'the dialog did not open');
  await h.pause();
}

async function create({ h, dialog, server }) {
  const before = server.of('new_workspace').length;
  await h.say('Create it');
  await h.tap(dialog.getByRole('button', { name: 'Create' }));
  await dialog.waitFor({ state: 'detached', timeout: 5000 });
  expectThat(server.of('new_workspace').length === before + 1, 'Create sent no new_workspace');
  return server.of('new_workspace').at(-1);
}

/** Source paths the dialog walkthroughs depend on (hashed by inputs-hash.mjs, DECISION-0158). */
const dialogInputs = [
  'packages/client/src/NewWorkspaceSheet.svelte',
  'packages/client/src/FolderPicker.svelte',
  'packages/client/e2e/video/**',
];

export const cases = {
  /** One tap on a folder used before. */
  'recent-folder': {
    title: 'One tap on a recent folder',
    group: 'dialog',
    page: 'docs/guide/workspaces.md',
    inputs: dialogInputs,
    // A returning user: the server's home was remembered from an earlier listing, so folders show as ~/…
    storage: { termenbox_server_homes: JSON.stringify({ one: HOME }), termenbox_recent_dirs: JSON.stringify({ one: [`${HOME}/code/termenbox`, `${HOME}/code/app`] }) },
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.still('dialog-open');
      await h.say('Folders you used before are one tap away', q('dir-chip').first());
      await q('dir-chip').first().waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('dir-chip').first());
      await h.say('The summary says what will be created', q('summary'));
      expectThat(/in ~\/code\/termenbox$/.test(await q('summary').innerText()), `summary was "${await q('summary').innerText()}"`);
      await h.still('folder-chosen');
      const made = await create(c);
      expectThat(made.dir === `${HOME}/code/termenbox`, `new_workspace dir was ${made.dir}`);
    },
  },

  /** An agent that is not one of the three tiles. */
  'other-agent': {
    title: 'Choose an agent that is not a tile',
    group: 'dialog',
    page: 'docs/guide/workspaces.md',
    inputs: dialogInputs,
    storage: {},
    async run(c) {
      const { h, q, dialog } = c;
      await openDialog(c);
      await h.say('Three agents are one tap; the rest are under More', q('agents-more'));
      await h.tap(q('agents-more'));
      await q('agents-layer').waitFor({ state: 'visible', timeout: 5000 });
      await h.say('Type a few letters to narrow the list', q('agents-layer').locator('input').first());
      await h.type(q('agents-layer').locator('input').first(), 'gem');
      const gemini = q('agents-layer').getByRole('option', { name: /Gemini/ });
      await gemini.waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(gemini);
      await q('agents-layer').waitFor({ state: 'detached', timeout: 5000 });
      expectThat(/^Gemini in /.test(await q('summary').innerText()), `summary was "${await q('summary').innerText()}"`);
      await h.say('Gemini is now the chosen agent', q('summary'));
      void dialog;
      const made = await create(c);
      expectThat(made.template === 'gemini', `new_workspace template was ${made.template}`);
    },
  },

  /** Walk the server's folders. */
  'browse-folder': {
    title: "Browse the server's folders",
    group: 'dialog',
    page: 'docs/guide/workspaces.md',
    inputs: dialogInputs,
    storage: {},
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.say('Choose the folder from your server', q('folder-change'));
      await h.tap(q('folder-change'));
      await q('picker-list').waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('picker-list').locator('[data-dir="code"]'));
      await q('picker-list').locator('[data-dir="termenbox"]').waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('picker-list').locator('[data-dir="termenbox"]'));
      await q('picker-list').locator('[data-dir="packages"]').waitFor({ state: 'visible', timeout: 5000 });
      await h.say('Select this folder', q('picker-select'));
      await h.tap(q('picker-select'));
      await q('picker-list').waitFor({ state: 'detached', timeout: 5000 });
      expectThat(/code\/termenbox$/.test(await q('folder-value').innerText()), `folder was "${await q('folder-value').innerText()}"`);
      const made = await create(c);
      expectThat(made.dir === `${HOME}/code/termenbox`, `new_workspace dir was ${made.dir}`);
    },
  },

  /** Paste or type a path. */
  'type-path': {
    title: 'Type or paste a path',
    group: 'dialog',
    page: 'docs/guide/workspaces.md',
    inputs: dialogInputs,
    storage: {},
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.say('Know the path? Type it in the folder picker', q('folder-change'));
      await h.tap(q('folder-change'));
      await q('picker-path').waitFor({ state: 'visible', timeout: 5000 });
      await h.type(q('picker-path'), `${HOME}/code/app`);
      await h.tap(q('picker-go'));
      await q('picker-trail').waitFor({ state: 'visible', timeout: 5000 });
      await h.say('Select this folder', q('picker-select'));
      await h.tap(q('picker-select'));
      await q('picker-path').waitFor({ state: 'detached', timeout: 5000 });
      expectThat(/code\/app$/.test(await q('folder-value').innerText()), `folder was "${await q('folder-value').innerText()}"`);
      const made = await create(c);
      expectThat(made.dir === `${HOME}/code/app`, `new_workspace dir was ${made.dir}`);
    },
  },
};

/** Opens the workspace named `name` the way the profile's user does: from the Workspaces list on a phone, the sidebar on desktop. */
async function openWorkspace({ h, page }, name) {
  if (h.phone) await h.tap(page.getByRole('button', { name: /^Workspaces/ }).first());
  const row = h.phone ? page.getByRole('button').filter({ hasText: new RegExp(`^\\s*${name}\\s+one`) }).first() : page.locator('[data-shell=sidebar] [data-key]').filter({ hasText: name }).first();
  await h.tap(row);
  await page.locator('.xterm').first().waitFor({ state: 'visible', timeout: 5000 });
}
const screenHas = (page, text) => page.waitForFunction((x) => [...document.querySelectorAll('.xterm-rows > div')].some((d) => d.textContent.includes(x)), text, { timeout: 8000 });

Object.assign(cases, {
  /** Add a server: paste the link, read what is being added, name it, see that this device can reach it. Real server binary. */
  'add-server': {
    title: 'Add your server',
    group: 'core',
    page: 'docs/guide/install.md',
    server: 'real',
    real: true,
    inputs: ['packages/client/src/AddServerModal.svelte', 'packages/client/src/addserver/**', 'packages/client/src/lib/addServer.ts', 'packages/client/e2e/video/**'],
    async run({ h, page, real }) {
      await h.say('Tap Add Server');
      await h.tap(page.getByRole('button', { name: 'Add Server', exact: true }).first());
      await page.getByRole('dialog').waitFor();
      await h.say('Step 1 installs the server; it announces itself while you run the command');
      await h.tap(page.getByTestId('install-next'));
      await h.say('Paste the link the installer printed', page.locator('#have-link'));
      await h.tap(page.locator('#have-link'));
      await page.locator('#have-link').fill(real.claim);
      await h.pause();
      await h.tap(page.getByRole('button', { name: 'Add', exact: true }).first());
      await page.getByText('This server is ready to add').waitFor({ timeout: 40000 });
      const group = page.getByRole('group', { name: 'What you are adding' });
      await group.scrollIntoViewIfNeeded();
      await h.say('Nothing is claimed yet: this says what you are adding', group);
      expectThat(/isolated container|on the machine itself/.test(await group.innerText()), 'the preview does not say where workspaces run');
      expectThat(!existsSync(join(real.config, 'owner.json')), 'the server was claimed before Add');
      await h.still('pairing-preview');
      await h.type(page.locator('#preview-name'), 'home-server');
      await h.tap(group.getByRole('button', { name: 'Add', exact: true }));
      await page.getByText('Server added').waitFor({ timeout: 20000 });
      await page.getByText(/This device can reach it|could not reach it|Could not tell/).first().waitFor({ timeout: 60000 });
      const panel = page.locator('.done[role=status]');
      await panel.scrollIntoViewIfNeeded();
      await h.say('Then it checks that this device can reach the server', panel);
      expectThat(/This device can reach it/.test(await panel.innerText()), `result panel said "${(await panel.innerText()).replace(/\s+/g, ' ').slice(0, 100)}"`);
      await h.still('server-added');
    },
  },

  /** The connection drops; the app reconnects and re-attaches to the workspace it had open (DECISION-0054). A page reload lands on Home. */
  'resume-after-close': {
    title: 'Lose the connection and keep your place',
    group: 'core',
    page: 'docs/guide/workspaces.md',
    server: 'scripted',
    inputs: ['packages/client/src/App.svelte', 'packages/client/src/lib/hub.ts', 'packages/client/src/lib/connection.ts', 'packages/client/e2e/video/**'],
    storage: {},
    async run(c) {
      const { h, page, server } = c;
      await h.say('Open a workspace');
      await openWorkspace(c, 'shell');
      await screenHas(page, 'attached to s3');
      expectThat(server.of('attach').at(-1)?.workspace_id === 's3', 'the first attach was not for the shell workspace');
      await h.say('Its screen is live on the server');
      server.output('npm run build\r\nbuild finished\r\n$ ');
      await screenHas(page, 'build finished');
      await h.pause(1500);
      await h.say('Lose the signal: a tunnel, a lock screen, a dropped Wi-Fi');
      await h.pause(1000);
      const attaches = server.of('attach').length;
      for (const ws of server.sockets) ws.close();
      await h.say('The app reconnects by itself and re-attaches');
      const back = Date.now() + 15000;
      while (server.of('attach').length === attaches && Date.now() < back) await page.waitForTimeout(100);
      expectThat(server.of('attach').length > attaches && server.of('attach').at(-1).workspace_id === 's3', 'the app did not re-attach to the shell workspace');
      await screenHas(page, 'attached to s3');
      server.output('build finished\\r\\n$ '); // the real server redraws the screen on attach; the scripted one only says it attached
      await screenHas(page, 'build finished');
      expectThat(await page.getByText('Connected').first().isVisible().catch(() => false) || h.phone, 'the server is not shown as connected again');
      await h.still('re-attached');
    },
  },
});
