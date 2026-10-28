import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, extname, resolve } from 'node:path';

const here = import.meta.dirname;
const dist = join(here, 'dist');
const SHOTS = join(dist, 'newworkspace-shots');
const mime = { '.html': 'text/html', '.js': 'application/javascript', '.css': 'text/css', '.png': 'image/png' };

const server = createServer((req, res) => {
  const file = join(dist, req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const port = server.address().port;
const url = `http://127.0.0.1:${port}/NewWorkspaceSheetGallery.html`;

console.log(`Serving on ${url}`);

const browser = await chromium.launch({ headless: true });
mkdirSync(SHOTS, { recursive: true });

// Mobile viewport (iPhone 12/13/14)
const mobileContext = await browser.newContext({
  viewport: { width: 390, height: 844 },
  deviceScaleFactor: 3,
  hasTouch: true,
  isMobile: true,
});
const mobilePage = await mobileContext.newPage();
await mobilePage.goto(url);
await mobilePage.waitForLoadState('networkidle');
await mobilePage.waitForSelector('dialog[aria-label="New workspace"]', { timeout: 10000 });
await mobilePage.waitForTimeout(500); // Let animations settle

// Full page mobile
await mobilePage.screenshot({ path: join(SHOTS, 'newworkspace-mobile-full.png'), fullPage: true });
console.log('Saved mobile full page screenshot');

// Dialog only mobile
const mobileDialog = mobilePage.locator('dialog[aria-label="New workspace"]');
await mobileDialog.screenshot({ path: join(SHOTS, 'newworkspace-mobile-dialog.png') });
console.log('Saved mobile dialog screenshot');

// Test "Show all agents" button on mobile - opens PickerSheet
const showAllAgentsBtnMobile = mobilePage.locator('button:has-text("Show all")').first();
if (await showAllAgentsBtnMobile.isVisible()) {
  await showAllAgentsBtnMobile.click();
  await mobilePage.waitForTimeout(500);
  // PickerSheet uses Dialog from bits-ui - look for the dialog
  const agentPickerSheet = mobilePage.locator('[role="dialog"]:has-text("Choose agent")');
  if (await agentPickerSheet.isVisible()) {
    await agentPickerSheet.screenshot({ path: join(SHOTS, 'newworkspace-mobile-agent-picker.png') });
    console.log('Saved mobile agent picker sheet screenshot');
  }
  // Close the picker
  await mobilePage.keyboard.press('Escape');
  await mobilePage.waitForTimeout(300);
}

// Test "+N more" directory chips button on mobile
const showMoreDirsBtnMobile = mobilePage.locator('button.chip.show-more:has-text("more")');
if (await showMoreDirsBtnMobile.isVisible()) {
  await showMoreDirsBtnMobile.click();
  await mobilePage.waitForTimeout(500);
  const dirPickerSheet = mobilePage.locator('[role="dialog"]:has-text("Recent directories")');
  if (await dirPickerSheet.isVisible()) {
    await dirPickerSheet.screenshot({ path: join(SHOTS, 'newworkspace-mobile-dir-picker.png') });
    console.log('Saved mobile directory picker sheet screenshot');
  }
  await mobilePage.keyboard.press('Escape');
  await mobilePage.waitForTimeout(300);
}

await mobileContext.close();

// Desktop viewport
const desktopContext = await browser.newContext({
  viewport: { width: 1280, height: 800 },
  deviceScaleFactor: 1,
});
const desktopPage = await desktopContext.newPage();
await desktopPage.goto(url);
await desktopPage.waitForLoadState('networkidle');
await desktopPage.waitForSelector('dialog[aria-label="New workspace"]', { timeout: 10000 });
await desktopPage.waitForTimeout(500);

// Full page desktop
await desktopPage.screenshot({ path: join(SHOTS, 'newworkspace-desktop-full.png'), fullPage: true });
console.log('Saved desktop full page screenshot');

// Dialog only desktop
const desktopDialog = desktopPage.locator('dialog[aria-label="New workspace"]');
await desktopDialog.screenshot({ path: join(SHOTS, 'newworkspace-desktop-dialog.png') });
console.log('Saved desktop dialog screenshot');

// Test "Browse..." button visibility on desktop
const browseBtn = desktopPage.locator('button:has-text("Browse")');
if (await browseBtn.isVisible()) {
  console.log('Browse button is visible on desktop');
  await desktopDialog.screenshot({ path: join(SHOTS, 'newworkspace-desktop-dialog-browse.png') });
  console.log('Saved desktop dialog with browse button screenshot');
}

// Test "Show all agents" button on desktop - opens PickerSheet (centered modal)
const showAllAgentsBtnDesktop = desktopPage.locator('button:has-text("Show all")').first();
if (await showAllAgentsBtnDesktop.isVisible()) {
  await showAllAgentsBtnDesktop.click();
  await desktopPage.waitForTimeout(500);
  const agentPickerSheet = desktopPage.locator('[role="dialog"]:has-text("Choose agent")');
  if (await agentPickerSheet.isVisible()) {
    await agentPickerSheet.screenshot({ path: join(SHOTS, 'newworkspace-desktop-agent-picker.png') });
    console.log('Saved desktop agent picker sheet screenshot');
  }
  await desktopPage.keyboard.press('Escape');
  await desktopPage.waitForTimeout(300);
}

// Test "+N more" directory chips button on desktop
const showMoreDirsBtnDesktop = desktopPage.locator('button.chip.show-more:has-text("more")');
if (await showMoreDirsBtnDesktop.isVisible()) {
  await showMoreDirsBtnDesktop.click();
  await desktopPage.waitForTimeout(500);
  const dirPickerSheet = desktopPage.locator('[role="dialog"]:has-text("Recent directories")');
  if (await dirPickerSheet.isVisible()) {
    await dirPickerSheet.screenshot({ path: join(SHOTS, 'newworkspace-desktop-dir-picker.png') });
    console.log('Saved desktop directory picker sheet screenshot');
  }
  await desktopPage.keyboard.press('Escape');
  await desktopPage.waitForTimeout(300);
}

await desktopContext.close();
await browser.close();
server.close();

console.log(`All screenshots saved to ${SHOTS}`);