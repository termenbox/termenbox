// The real App.svelte (built by `vite build` into dist/) signed in with a forged local certificate and one server that is
// not reachable: no server binary is needed. Checks that the four destinations, the Servers page (part of Settings), the
// palette and the address bar work in the whole app, at a phone and a desktop width, without a page error.
// Usage: node e2e/app.mjs (after: pnpm run e2e:build)
import { launch, contextOptions } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';
import { ScriptedServer, startApp } from './app-lib.mjs';

const dist = join(import.meta.dirname, '../dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const srv = createServer((req, res) => {
 let f = join(dist, req.url.split('?')[0].split('#')[0]);
 if (!f.startsWith(dist) || !existsSync(f) || statSync(f).isDirectory()) f = join(dist, 'index.html');
 res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${srv.address().port}/`;
const browser = await launch();
let failed = false;
const fail = (m) => { failed = true; console.error(`FAIL: ${m}`); };
const check = (c, m) => { if (!c) fail(m); };
const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64').replace(/\+/g, '-').replace(/\//g, '_');
const cert = `${b64({ exp: Math.floor(Date.now() / 1000) + 86400, sub: 1 })}.sig`;

for (const [name, opts] of [['phone 360', { viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true }], ['phone 390', { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }], ['desktop 1280', { viewport: { width: 1280, height: 800 }, isMobile: false, hasTouch: false }]]) {
 const ctx = await browser.newContext(contextOptions(opts));
 await ctx.addInitScript(([c]) => {
   localStorage.setItem('termenbox_auth', JSON.stringify({ cert: c, login: 'tester' }));
   localStorage.setItem('termenbox_servers', JSON.stringify([{ server_id: 'one', server_name: 'one', server_url: 'ws://127.0.0.1:59999/ws', registered_at: 0 }])); // an unused port: WebKit throws at once for the blocked low ports (9), Chromium fails later
   localStorage.setItem('termenbox_ios_hint', '1');
 }, [cert]);
 const page = await ctx.newPage();
 const errors = [];
 page.on('pageerror', (e) => errors.push(String(e)));
 await page.goto(origin);
 const wide = opts.viewport.width > 720;
 const nav = page.getByRole('navigation', { name: 'Main' });
 await nav.waitFor();
 const go = async (id, heading) => {
   await nav.locator(`[data-destination=${id}]`).click();
   await page.getByRole('heading', { name: heading, exact: true }).waitFor();
   check(new URL(page.url()).hash === `#/${id}`, `${name}: address bar is ${new URL(page.url()).hash}, not #/${id}`);
 };
 await go('home', 'Home');
 await go('activity', 'Activity');
 await go('settings', 'Settings');
 // Privacy and your data: the notice, then a delete that needs a confirmation, shows a plain failure, and can be retried.
 let deletes = 0;
 await page.route('**/api/me/delete', (route) => {
   deletes++;
   return deletes === 1 ? route.fulfill({ status: 401, json: { error: 'bad certificate' } }) : route.fulfill({ json: { ok: true, servers: 2 } });
 });
 await page.getByRole('button', { name: /^Privacy and your data/ }).click();
 await page.getByRole('heading', { name: 'Privacy and your data', exact: true }).waitFor();
 check((await page.getByText('30 days').count()) > 0, `${name}: the 30-day retention is not stated`);
 await page.getByRole('button', { name: 'Delete my data…' }).click();
 check(deletes === 0, `${name}: the first tap already deleted`);
 await page.getByRole('button', { name: 'Keep my data' }).click();
 await page.getByRole('button', { name: 'Delete my data…' }).click();
 check(await page.evaluate(() => document.activeElement?.textContent?.includes('Delete your records')), `${name}: focus did not move to the confirmation`);
 const row = (await page.getByRole('button', { name: 'Delete my data', exact: true }).boundingBox());
 const rowKeep = await page.getByRole('button', { name: 'Keep my data' }).boundingBox();
 check(row && rowKeep && row.height <= 48 && rowKeep.height <= 48 && row.x + row.width <= page.viewportSize().width, `${name}: the confirm buttons wrap or overflow`);
 await page.getByRole('button', { name: 'Delete my data', exact: true }).click();
 await page.getByRole('alert').filter({ hasText: 'sign-in has expired' }).waitFor();
 check((await page.getByRole('button', { name: 'Try again' }).count()) === 0, `${name}: an expired sign-in offers a retry that cannot work`);
 await page.getByRole('button', { name: 'Sign out' }).waitFor();
 await page.getByRole('button', { name: 'Close' }).click();
 await page.getByRole('button', { name: 'Delete my data…' }).click();
 await page.getByRole('button', { name: 'Delete my data', exact: true }).click();
 await page.getByText('Removed the records of 2 servers').waitFor();
 check(deletes === 2, `${name}: expected 2 delete requests, saw ${deletes}`);
 await page.locator('[data-shell=privacy] button').first().click();
 await page.getByRole('heading', { name: 'Settings', exact: true }).waitFor();
 // The Servers page belongs to Settings and Back leaves it.
 await page.getByRole('button', { name: /^Servers/ }).first().click();
 await page.getByText('Add server', { exact: false }).first().waitFor({ timeout: 5000 }).catch(() => fail(`${name}: the Servers page did not open from Settings`));
 await nav.locator('[data-destination=home]').click();
 await page.getByRole('heading', { name: 'Home', exact: true }).waitFor();
 await nav.locator('[data-destination=workspaces]').click();
 check((await page.getByRole('heading', { name: 'Home', exact: true }).count()) === 0, `${name}: Workspaces still shows Home`);
 check((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0, `${name}: horizontal overflow`);
 check((await page.locator('[data-shell=sidebar]').count()) === (wide ? 1 : 0), `${name}: sidebar presence is wrong`);
 check((await page.locator('[data-shell=bottom-bar]').count()) === (wide ? 0 : 1), `${name}: bottom bar presence is wrong`);
 // The palette opens from the keyboard and jumps to a destination.
 await page.keyboard.press('Control+k');
 await page.getByRole('combobox', { name: 'Search' }).waitFor();
 await page.keyboard.type('activity');
 await page.keyboard.press('Enter');
 await page.getByRole('heading', { name: 'Activity', exact: true }).waitFor();
 await page.goBack();
 await page.getByRole('heading', { name: 'Activity', exact: true }).waitFor({ state: 'detached' });
 // The Menu is a bottom sheet and must stay in the visual viewport at every width.
 await page.getByRole('button', { name: 'Menu' }).click();
 const menu = page.getByRole('dialog', { name: 'Menu' });
 await menu.waitFor();
 const menuBox = await menu.boundingBox();
 check(menuBox && menuBox.y >= 0 && menuBox.y + menuBox.height <= page.viewportSize().height + 1, `${name}: Menu is outside the viewport`);
 check(await menu.getByRole('button', { name: 'Log out' }).isVisible(), `${name}: Menu actions are not visible`);
 check(await menu.evaluate((e) => e.scrollWidth <= e.clientWidth + 1), `${name}: Menu scrolls horizontally`);
 await menu.getByRole('button', { name: 'Close menu' }).click();
 check(errors.length === 0, `${name}: page errors: ${errors.slice(0, 3).join(' | ')}`);
 console.log(`ok  ${name}: the whole app moves between all four destinations, the Servers page and the palette`);
 await ctx.close();
}
// The actual Servers page and hub: a status notice, cancelled confirmation, failed helper and successful reconnect.
class UpdateServer extends ScriptedServer {
 version = '0.1.53+old';
 available = '';
 handle(ws, m) {
   if (m.type === 'auth') {
     this.received.push(m);
     this.send(ws, { type: 'auth_ok', protocol: 2, tabs: true, user: 'tester', server_name: 'one', version: this.version, update_available: this.available, backends: ['tmux'] });
     this.send(ws, { type: 'workspaces', workspaces: this.workspaces() });
     this.send(ws, this.snapshot());
   } else if (m.type === 'update_server') {
     this.received.push(m);
     this.send(ws, { type: 'update_requested', available: this.available });
   } else super.handle(ws, m);
 }
}
for (const [label, viewport] of [['phone', { width: 390, height: 844 }], ['desktop', { width: 1280, height: 800 }]]) {
 const server = new UpdateServer();
 const t = await startApp({ origin, browser }, { viewport, server });
 const page = t.page;
 try {
   await t.open();
   await page.getByRole('navigation', { name: 'Main' }).locator('[data-destination=settings]').click();
   await page.getByRole('button', { name: /^Servers/ }).first().click();
   await page.getByTestId('server-version').filter({ hasText: 'v0.1.53' }).waitFor();
   check(await page.getByTestId('server-update').count() === 0, `${label}: up-to-date server offers an update`);
   server.available = '0.1.54'; server.broadcast({ type: 'update_status', available: server.available });
   const update = page.getByTestId('server-update');
   await update.waitFor();
   page.once('dialog', d => d.dismiss()); await update.click();
   check(server.of('update_server').length === 0, `${label}: cancelled confirmation sent an update`);
   page.once('dialog', d => d.accept()); await update.click();
   await page.getByTestId('server-updating').waitFor();
   check(server.of('update_server').length === 1, `${label}: expected one update request`);
   check(await update.count() === 0, `${label}: update can be clicked again while pending`);
   server.broadcast({ type: 'error', message: 'An unrelated folder request failed' });
   await page.getByText('An unrelated folder request failed', { exact: true }).first().waitFor();
   check(await page.getByTestId('server-updating').count() === 1, `${label}: unrelated error ended the update`);
   server.broadcast({ type: 'error', message: 'Server update: The update failed and was rolled back.' });
   await page.getByTestId('server-updating').waitFor({ state: 'hidden' });
   await update.waitFor();
   page.once('dialog', d => d.accept()); await update.click();
   await page.getByTestId('server-updating').waitFor();
   server.version = '0.1.54+new'; server.available = '';
   for (const ws of server.sockets) ws.close();
   await page.getByTestId('server-version').filter({ hasText: 'v0.1.54' }).waitFor({ timeout: 30_000 });
   check(await page.getByTestId('server-updating').count() === 0, `${label}: newer version still says Updating`);
   check(await update.count() === 0, `${label}: newer version still offers the installed update`);
   check(server.of('update_server').length === 2, `${label}: retry was duplicated or lost`);
 } catch (e) { t.fail(String(e)); }
 if (!(await t.end())) failed = true;
}

// New tab is another bottom sheet. Use a tab-capable scripted server at a short desktop height and simulate
// the visual viewport being above the keyboard; the same CSS variables are written by AppShell on a real phone.
class TabbedServer extends ScriptedServer {
 workspaces() {
   return [{ id: 's1', name: 'a very long workspace name that must wrap instead of leaving the sheet', agent: null,
     tabs: [{ id: 't1', label: 'Shell', kind: 'shell', dir: '', order: 0, window: '@0' }] }];
 }
 snapshot() {
   return { type: 'agent_snapshot', states: [{ workspace: 's1', state: 'idle', seq: 1, ts: Date.now(), stale: false, agent: null }] };
 }
 handle(ws, m) {
   if (m.type === 'auth') {
     this.received.push(m);
     this.send(ws, { type: 'auth_ok', protocol: 2, tabs: true, user: 'tester', server_name: 'one', version: '0.1.31', backends: ['tmux'], control_mode: true, prefix: 'C-b' });
     this.send(ws, { type: 'workspaces', workspaces: this.workspaces() });
     this.send(ws, this.snapshot());
     return;
   }
   super.handle(ws, m);
 }
}
{
 const t = await startApp({ origin, browser }, { viewport: { width: 1280, height: 420 }, server: new TabbedServer() });
 const page = t.page;
 try {
   await t.open();
   await t.openWorkspace('s1');
   await page.evaluate(() => {
     const s = document.documentElement.style;
     s.setProperty('--app-height', '240px');
     s.setProperty('--app-bottom', '180px');
     s.setProperty('--app-top', '0px');
     s.setProperty('--app-width', '1280px');
   });
   await page.getByTestId('strip-new-tab').click();
   const sheet = page.getByRole('dialog', { name: 'New tab' });
   await sheet.waitFor();
   const box = await sheet.boundingBox();
   t.check(box && box.y >= 0 && box.y + box.height <= 241, 'New tab is outside the visual viewport');
   t.check(await sheet.locator('h2').evaluate((e) => e.scrollWidth <= e.clientWidth + 1), 'New tab title scrolls horizontally');
   await sheet.getByRole('button', { name: 'Create' }).scrollIntoViewIfNeeded();
   const action = await sheet.getByRole('button', { name: 'Create' }).boundingBox();
   t.check(action && action.y >= 0 && action.y + action.height <= 241, 'New tab actions are not reachable');
   t.check(await sheet.evaluate((e) => e.scrollWidth <= e.clientWidth + 1), 'New tab scrolls horizontally');
   await sheet.getByRole('button', { name: 'Cancel' }).click();
   await page.getByRole('button', { name: 'Menu' }).click();
   const menu = page.getByRole('dialog', { name: 'Menu' });
   await menu.waitFor();
   const menuBox = await menu.boundingBox();
   t.check(menuBox && menuBox.y >= 0 && menuBox.y + menuBox.height <= 241, 'Menu does not follow the simulated keyboard inset');
   await menu.getByRole('button', { name: 'Close menu' }).click();
   t.ok('desktop short viewport: Menu and New tab stay inside the visual viewport');
 } catch (e) { t.fail(String(e)); }
 if (!(await t.end())) failed = true;
}
await browser.close();
srv.close();
if (failed) process.exit(1);
console.log('PASS');
