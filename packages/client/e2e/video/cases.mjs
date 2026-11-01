// The four New workspace dialog walkthroughs (DECISION-0148). Each is a function of `c` (see render.mjs) and ends with an
// assertion of its final state, so a walkthrough that did not reach it fails instead of producing a video of something else
// (GUARDRAIL ux-scenarios-assert-a-state-before-they-capture-it). They run on the scripted server and its fake home.
import { HOME } from '../fake-tree.mjs';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { noteAssertion } from './lib.mjs';
import { checkDeclared } from './steps.mjs';

const expectThat = (cond, what) => { if (!cond) throw new Error(`final state: ${what}`); noteAssertion(); };

/** Opens the dialog the way the profile's user does: the visible New button, else the command palette. */
async function openDialog({ h, page, dialog, q }) {
  const plus = page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first();
  await h.step('open');
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
  await h.step('create');
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
    steps: [
      { id: 'open', caption: 'Start a new workspace', label: '+ New' },
      { id: 'pick-folder', caption: 'Folders you used before are one tap away', label: '~/code/termenbox' },
      { id: 'summary', caption: 'The summary says what will be created', label: null },
      { id: 'create', caption: 'Create it', label: 'Create' },
      { id: 'created', caption: 'Your workspace now runs in ~/code/termenbox', label: null },
    ],
    // A returning user: the server's home was remembered from an earlier listing, so folders show as ~/…
    storage: { termenbox_server_homes: JSON.stringify({ one: HOME }), termenbox_recent_dirs: JSON.stringify({ one: [`${HOME}/code/termenbox`, `${HOME}/code/app`] }) },
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.still('poster');
      await h.step('pick-folder', q('dir-chip').first());
      await q('dir-chip').first().waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('dir-chip').first());
      await h.step('summary', q('summary'));
      expectThat(/in ~\/code\/termenbox$/.test(await q('summary').innerText()), `summary was "${await q('summary').innerText()}"`);
      await h.still('folder-chosen');
      const made = await create(c);
      expectThat(made.dir === `${HOME}/code/termenbox`, `new_workspace dir was ${made.dir}`);
      await h.step('created');
      await h.result();
    },
  },

  /** An agent that is not one of the three tiles. */
  'other-agent': {
    title: 'Choose an agent that is not a tile',
    group: 'dialog',
    page: 'docs/guide/workspaces.md',
    inputs: dialogInputs,
    steps: [
      { id: 'open', caption: 'Start a new workspace', label: '+ New' },
      { id: 'more', caption: 'Three agents are one tap; the rest are under More', label: 'More' },
      { id: 'narrow', caption: 'Type a few letters to narrow the list', label: null },
      { id: 'chosen', caption: 'Gemini is now the chosen agent', label: null },
      { id: 'create', caption: 'Create it', label: 'Create' },
    ],
    storage: {},
    async run(c) {
      const { h, q, dialog } = c;
      await openDialog(c);
      await h.step('more', q('agents-more'));
      await h.tap(q('agents-more'));
      await q('agents-layer').waitFor({ state: 'visible', timeout: 5000 });
      await h.step('narrow', q('agents-layer').locator('input').first());
      await h.type(q('agents-layer').locator('input').first(), 'gem');
      const gemini = q('agents-layer').getByRole('option', { name: /Gemini/ });
      await gemini.waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(gemini);
      await q('agents-layer').waitFor({ state: 'detached', timeout: 5000 });
      expectThat(/^Gemini in /.test(await q('summary').innerText()), `summary was "${await q('summary').innerText()}"`);
      await h.step('chosen', q('summary'));
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
    steps: [
      { id: 'open', caption: 'Start a new workspace', label: '+ New' },
      { id: 'browse', caption: 'Choose the folder from your server', label: 'Choose' },
      { id: 'select', caption: 'Select this folder', label: 'Select this folder' },
      { id: 'create', caption: 'Create it', label: 'Create' },
    ],
    storage: {},
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.step('browse', q('folder-change'));
      await h.tap(q('folder-change'));
      await q('picker-list').waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('picker-list').locator('[data-dir="code"]'));
      await q('picker-list').locator('[data-dir="termenbox"]').waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('picker-list').locator('[data-dir="termenbox"]'));
      await q('picker-list').locator('[data-dir="packages"]').waitFor({ state: 'visible', timeout: 5000 });
      await h.step('select', q('picker-select'));
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
    steps: [
      { id: 'open', caption: 'Start a new workspace', label: '+ New' },
      { id: 'browse', caption: 'Know the path? Type it in the folder picker', label: 'Choose' },
      { id: 'select', caption: 'Select this folder', label: 'Select this folder' },
      { id: 'create', caption: 'Create it', label: 'Create' },
    ],
    storage: {},
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.step('browse', q('folder-change'));
      await h.tap(q('folder-change'));
      await q('picker-path').waitFor({ state: 'visible', timeout: 5000 });
      await h.type(q('picker-path'), `${HOME}/code/app`);
      await h.tap(q('picker-go'));
      await q('picker-trail').waitFor({ state: 'visible', timeout: 5000 });
      await h.step('select', q('picker-select'));
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
  /** Install: the real installer's captured output replayed in a terminal (fixtures/, scripts/capture-installer-output.sh). */
  'install': {
    title: 'Install Termenbox on your server',
    group: 'start',
    page: 'docs/course/01-install.md',
    server: 'replay',
    replay: true,
    disclaimer: "A rendered replay of the real installer's output, not a live install",
    inputs: ['packages/client/installer/install.sh', 'packages/client/e2e/video/**'],
    steps: [
      { id: 'open', caption: 'On the machine that will run your agents, open a terminal', label: null },
      { id: 'paste', caption: 'Paste the install command and press Enter', label: null },
      { id: 'wait', caption: 'Wait for the QR code and the link', label: null },
    ],
    async run({ h, replay }) {
      await h.step('open', replay.bottom());
      await replay.prompt();
      await h.pause();
      await h.step('paste', replay.bottom());
      await replay.paste('curl -fsSL https://app.termenbox.dev/i | sh');
      await h.pause(1200);
      await replay.enter();
      await h.step('wait', replay.bottom());
      await replay.play(h.wait);
      expectThat(await replay.waitForLine(/^\s*https:\/\/app\.termenbox\.dev\/#\/pair\/\S+$/), 'the terminal does not show the pairing link line');
      expectThat(await replay.waitForLine(/^Scan this, open the link/), 'the terminal does not say to scan the code');
      await h.still('poster');
      await h.result();
    },
  },

  /** Add a server: paste the link, read what is being added, name it, see that this device can reach it. Real server binary. */
  'add-server': {
    title: 'Add your server',
    group: 'core',
    page: 'docs/guide/install.md',
    server: 'real',
    real: true,
    inputs: ['packages/client/src/AddServerModal.svelte', 'packages/client/src/addserver/**', 'packages/client/src/lib/addServer.ts', 'packages/client/e2e/video/**'],
    steps: [
      { id: 'tap-add', caption: 'Open the app and add a server', label: 'Add Server' },
      { id: 'install', caption: 'Step 1 installs the server; it announces itself while you run the command', label: 'Next' },
      { id: 'paste', caption: 'Paste the link the installer printed', label: null },
      { id: 'preview', caption: 'Nothing is claimed yet: this says what you are adding', label: null },
      { id: 'reach', caption: 'Then it checks that this device can reach the server', label: null },
    ],
    async run({ h, page, real }) {
      await h.step('tap-add');
      await h.tap(page.getByRole('button', { name: 'Add Server', exact: true }).first());
      await page.getByRole('dialog').waitFor();
      await h.step('install');
      await h.tap(page.getByTestId('install-next'));
      await h.step('paste', page.locator('#have-link'));
      await h.tap(page.locator('#have-link'));
      await page.locator('#have-link').fill(real.claim);
      await h.pause();
      await h.tap(page.getByRole('button', { name: 'Add', exact: true }).first());
      await page.getByText('This server is ready to add').waitFor({ timeout: 40000 });
      const group = page.getByRole('group', { name: 'What you are adding' });
      await group.scrollIntoViewIfNeeded();
      await h.step('preview', group);
      expectThat(/isolated container|on the machine itself/.test(await group.innerText()), 'the preview does not say where workspaces run');
      expectThat(!existsSync(join(real.config, 'owner.json')), 'the server was claimed before Add');
      await h.still('poster');
      await h.result();
      await h.type(page.locator('#preview-name'), 'home-server');
      await h.tap(group.getByRole('button', { name: 'Add', exact: true }));
      await page.getByText('Server added').waitFor({ timeout: 20000 });
      await page.getByText(/This device can reach it|could not reach it|Could not tell/).first().waitFor({ timeout: 60000 });
      const panel = page.locator('.done[role=status]');
      await panel.scrollIntoViewIfNeeded();
      await h.step('reach', panel);
      expectThat(/This device can reach it/.test(await panel.innerText()), `result panel said "${(await panel.innerText()).replace(/\s+/g, ' ').slice(0, 100)}"`);
      await h.still('server-added');
      await h.result();
    },
  },

  /** The connection drops; the app reconnects and re-attaches to the workspace it had open (DECISION-0054). A page reload lands on Home. */
  'resume-after-close': {
    title: 'Lose the connection and keep your place',
    group: 'core',
    page: 'docs/guide/workspaces.md',
    server: 'scripted',
    inputs: ['packages/client/src/App.svelte', 'packages/client/src/lib/hub.ts', 'packages/client/src/lib/connection.ts', 'packages/client/e2e/video/**'],
    steps: [
      { id: 'open', caption: 'Open a workspace', label: 'shell' },
      { id: 'live', caption: 'Its screen is live on the server', label: null },
      { id: 'lose', caption: 'Lose the signal: a tunnel, a lock screen, a dropped Wi-Fi', label: null },
      { id: 'back', caption: 'The app reconnects by itself and re-attaches', label: null },
    ],
    storage: {},
    async run(c) {
      const { h, page, server } = c;
      await h.step('open');
      await openWorkspace(c, 'shell');
      await screenHas(page, 'attached to s3');
      expectThat(server.of('attach').at(-1)?.workspace_id === 's3', 'the first attach was not for the shell workspace');
      await h.step('live');
      server.output('npm run build\r\nbuild finished\r\n$ ');
      await screenHas(page, 'build finished');
      await h.pause(1500);
      await h.step('lose');
      await h.pause(1000);
      const attaches = server.of('attach').length;
      for (const ws of server.sockets) ws.close();
      await h.step('back');
      const back = Date.now() + 15000;
      while (server.of('attach').length === attaches && Date.now() < back) await page.waitForTimeout(100);
      expectThat(server.of('attach').length > attaches && server.of('attach').at(-1).workspace_id === 's3', 'the app did not re-attach to the shell workspace');
      await screenHas(page, 'attached to s3');
      server.output('build finished\\r\\n$ '); // the real server redraws the screen on attach; the scripted one only says it attached
      await screenHas(page, 'build finished');
      expectThat(await page.getByRole('img', { name: /: Connected$/ }).first().isVisible().catch(() => false) || h.phone, 'the server is not shown as connected again');
      await h.still('poster');
      await h.result();
    },
  },
});

const typedBytes = (m) => Buffer.from(m.data, 'base64').toString('latin1');

Object.assign(cases, {
  /** Chapter 4: type a prompt to the agent, read its answer, use the key row for a key the phone keyboard lacks. */
  'talk-to-agent': {
    title: 'Talk to your agent',
    group: 'start',
    page: 'docs/course/04-talk-to-your-agent.md',
    server: 'scripted',
    inputs: ['packages/client/src/App.svelte', 'packages/client/src/Terminal.svelte', 'packages/client/src/KeyRow.svelte', 'packages/client/e2e/app-lib.mjs', 'packages/client/e2e/video/**'],
    steps: [
      { id: 'open', caption: 'Open the workspace your agent runs in', label: 'build' },
      { id: 'type', caption: 'Tap the screen and type what you want done', label: null },
      { id: 'send', caption: 'Send it with Enter', label: null },
      { id: 'answer', caption: 'The answer arrives on the same screen', label: null },
      { id: 'escape', caption: 'A key your phone keyboard lacks is in the key row: Escape stops the agent', label: 'Escape' },
    ],
    storage: {},
    // The agent is idle at its prompt; the server echoes what is typed and answers when Enter arrives, as tmux and the agent would.
    prepare(server) {
      server.states = { s1: 'idle', s2: 'idle', s3: 'idle' };
      server.views.set('s1', server.view('s1', { tail: ['$'] }));
      const handle = server.handle.bind(server);
      server.handle = (ws, m) => {
        if (m.type !== 'input') return handle(ws, m);
        server.received.push(m);
        const key = typedBytes(m);
        if (key === '\r') server.output('\r\n● Looking at the login test…\r\n  The token was not refreshed.\r\n  Fixed. The test passes.\r\n> ');
        else if (key === '\x1b') server.output('\r\n[Stopped]\r\n> ');
        else server.output(key);
      };
    },
    async run(c) {
      const { h, page, server } = c;
      await h.step('open');
      await openWorkspace(c, 'build');
      await screenHas(page, 'attached to s1');
      await h.step('type', page.locator('.xterm').first());
      await h.tap(page.locator('.xterm').first());
      const prompt = 'fix the login test';
      await h.typeText(prompt, (ch) => page.keyboard.type(ch));
      await screenHas(page, prompt);
      expectThat(server.of('input').map(typedBytes).join('') === prompt, `the server received "${server.of('input').map(typedBytes).join('')}"`);
      await h.pause();
      await h.step('send', page.locator('.xterm').first());
      await page.keyboard.press('Enter');
      await h.step('answer', page.locator('.xterm').first());
      await screenHas(page, 'The test passes');
      expectThat(await page.locator('.xterm-rows').innerText().then((t) => t.includes('Fixed. The test passes.')), 'the agent answer is not on screen');
      await h.still('poster');
      await h.result();
      const keys = page.getByRole('button', { name: 'Escape', exact: true });
      await h.step('escape', keys);
      await h.tap(keys);
      await screenHas(page, '[Stopped]');
      expectThat(server.of('input').some((m) => typedBytes(m) === '\x1b'), 'Escape did not reach the server');
      expectThat(await page.locator('.xterm-rows').innerText().then((t) => t.includes('The test passes.')), 'the answer left the screen');
    },
  },

  /** Chapter 5: the agent asks, the banner names the workspace, one tap answers, the status goes to Working. */
  'answer-from-the-train': {
    title: 'Answer your agent from anywhere',
    group: 'start',
    page: 'docs/course/05-answer-from-the-train.md',
    server: 'scripted',
    inputs: ['packages/client/src/App.svelte', 'packages/client/src/lib/decision.ts', 'packages/client/src/lib/notify.ts', 'packages/client/e2e/app-lib.mjs', 'packages/client/e2e/video/**'],
    steps: [
      { id: 'away', caption: 'You left your desk; your agent keeps working', label: null },
      { id: 'banner', caption: 'When it needs you, a banner names the workspace and offers its choices', label: null },
      { id: 'answer', caption: 'Tap your answer; it goes to the tab that asked', label: 'Yes' },
      { id: 'working', caption: 'The agent takes it and goes back to Working', label: null },
    ],
    storage: {},
    prepare(server) {
      server.states = { s1: 'working', s2: 'idle', s3: 'idle' };
      server.views.set('s1', server.view('s1', { tail: ['working'] }));
      const handle = server.handle.bind(server);
      // The banner and the card answer with `answer` (the workspace view uses workspace_answer): the agent takes it and works on.
      server.handle = (ws, m) => {
        if (m.type !== 'answer') return handle(ws, m);
        server.received.push(m);
        server.states[m.workspace] = 'working';
        server.broadcast({ type: 'agent_events', events: [{ workspace: m.workspace, seq: 4, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'state', state: 'working' }] });
      };
    },
    async run(c) {
      const { h, page, server } = c;
      const tile = (label, n) => page.getByRole('button', { name: new RegExp(`^\\s*${n}\\s*\\S?\\s*${label}`) });
      await tile('Working', 1).waitFor({ timeout: 8000 });
      expectThat(await tile('Needs you', 0).isVisible(), 'something already needed an answer');
      await h.step('away');
      await h.pause();
      // The agent asks (a hook reports the question with its choices).
      server.states.s1 = 'needs_input';
      server.broadcast({ type: 'agent_events', events: [
        { workspace: 's1', seq: 2, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'state', state: 'needs_input' },
        { workspace: 's1', seq: 3, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'prompt', id: 'p-s1', prompt_kind: 'choice', choices: ['Yes', 'No'] },
      ] });
      const yes = page.getByRole('button', { name: 'Yes', exact: true }).first();
      await yes.waitFor({ state: 'visible', timeout: 8000 });
      await h.step('banner', yes);
      expectThat(await page.getByText('build', { exact: false }).first().isVisible(), 'the banner does not name the workspace');
      await h.still('poster');
      await h.result();
      await h.step('answer', yes);
      await h.tap(yes);
      await tile('Working', 1).waitFor({ timeout: 8000 });
      await h.step('working');
      expectThat(await tile('Working', 1).isVisible() && await tile('Needs you', 0).isVisible(), 'the status did not go to Working');
      expectThat(server.of('answer').length === 1 && server.of('answer')[0].choice === 0, `the answer sent was ${JSON.stringify(server.of('answer'))}`);
      await h.still('working');
      await h.result();
    },
  },
});

for (const [id, c] of Object.entries(cases)) checkDeclared(id, c.steps);
