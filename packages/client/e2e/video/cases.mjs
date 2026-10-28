// The four New workspace dialog walkthroughs (DECISION-0141). Each is a function of `c` (see render.mjs) and ends with an
// assertion of its final state, so a walkthrough that did not reach it fails instead of producing a video of something else
// (GUARDRAIL ux-scenarios-assert-a-state-before-they-capture-it). They run on the scripted server and its fake home.
import { HOME } from '../fake-tree.mjs';

const expectThat = (cond, what) => { if (!cond) throw new Error(`final state: ${what}`); };

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

export const cases = {
  /** One tap on a folder used before. */
  'recent-folder': {
    // A returning user: the server's home was remembered from an earlier listing, so folders show as ~/…
    storage: { termenbox_server_homes: JSON.stringify({ one: HOME }), termenbox_recent_dirs: JSON.stringify({ one: [`${HOME}/code/termenbox`, `${HOME}/code/app`] }) },
    async run(c) {
      const { h, q } = c;
      await openDialog(c);
      await h.say('Folders you used before are one tap away', q('dir-chip').first());
      await q('dir-chip').first().waitFor({ state: 'visible', timeout: 5000 });
      await h.tap(q('dir-chip').first());
      await h.say('The summary says what will be created', q('summary'));
      expectThat(/in ~\/code\/termenbox$/.test(await q('summary').innerText()), `summary was "${await q('summary').innerText()}"`);
      const made = await create(c);
      expectThat(made.dir === `${HOME}/code/termenbox`, `new_workspace dir was ${made.dir}`);
    },
  },

  /** An agent that is not one of the three tiles. */
  'other-agent': {
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
