import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import sharp from 'sharp';
import pngToIco from 'png-to-ico';

// One source mark for every platform; the generated icon and splash sets are not committed.
// Run by every sync:* script and by `pnpm --filter termenbox-native run build:assets`.

const brand = { r: 0x1a, g: 0x1a, b: 0x2e };
const svg = readFileSync('assets/icon.svg');

function put(file, buffer) {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, buffer);
}

async function logo(size) {
  return sharp(svg, { density: 512 })
    .resize(size, size, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();
}

async function icon(size, { padding = 0, transparent = false, circle = false, opaque = false } = {}) {
  const inner = Math.max(1, Math.round(size * (1 - padding * 2)));
  const art = await logo(inner);
  const background = transparent ? { r: 0, g: 0, b: 0, alpha: 0 } : { ...brand, alpha: 1 };
  let out = await sharp({ create: { width: size, height: size, channels: 4, background } })
    .composite([{ input: art, gravity: 'center' }])
    .png()
    .toBuffer();
  if (circle) {
    const mask = Buffer.from(
      `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}"><circle cx="${size / 2}" cy="${size / 2}" r="${size / 2}" fill="#fff"/></svg>`,
    );
    out = await sharp(out).composite([{ input: mask, blend: 'dest-in' }]).png().toBuffer();
  }
  if (opaque) out = await sharp(out).flatten({ background: brand }).png().toBuffer();
  return out;
}

async function splash(width, height) {
  const art = await logo(Math.round(Math.min(width, height) * 0.25));
  return sharp({ create: { width, height, channels: 3, background: brand } })
    .composite([{ input: art, gravity: 'center' }])
    .png()
    .toBuffer();
}

// Android launcher densities: 48dp base, adaptive foreground at 108dp.
const densities = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
for (const [density, scale] of Object.entries(densities)) {
  const dir = `android/app/src/main/res/mipmap-${density}`;
  const size = Math.round(48 * scale);
  put(`${dir}/ic_launcher.png`, await icon(size));
  put(`${dir}/ic_launcher_round.png`, await icon(size, { circle: true, padding: 0.06 }));
  put(`${dir}/ic_launcher_foreground.png`, await icon(Math.round(108 * scale), { transparent: true, padding: 0.17 }));
}

// The template's splash buckets and their sizes.
const splashes = {
  drawable: [480, 320],
  'drawable-land-mdpi': [480, 320],
  'drawable-land-hdpi': [800, 480],
  'drawable-land-xhdpi': [1280, 720],
  'drawable-land-xxhdpi': [1600, 960],
  'drawable-land-xxxhdpi': [1920, 1280],
  'drawable-port-mdpi': [320, 480],
  'drawable-port-hdpi': [480, 800],
  'drawable-port-xhdpi': [720, 1280],
  'drawable-port-xxhdpi': [960, 1600],
  'drawable-port-xxxhdpi': [1280, 1920],
};
for (const [dir, [width, height]] of Object.entries(splashes)) {
  put(`android/app/src/main/res/${dir}/splash.png`, await splash(width, height));
}

// iOS: one 1024 app icon (no alpha) and the 2732 splash imageset.
put('ios/App/App/Assets.xcassets/AppIcon.appiconset/AppIcon-512@2x.png', await icon(1024, { opaque: true }));
for (const name of ['splash-2732x2732-1.png', 'splash-2732x2732-2.png', 'splash-2732x2732.png']) {
  put(`ios/App/App/Assets.xcassets/Splash.imageset/${name}`, await splash(2732, 2732));
}

// Electron: window/tray icon, Windows .ico and the splash image.
put('electron/assets/appIcon.png', await icon(512));
put('electron/assets/splash.png', await splash(480, 800));
const icoSizes = [16, 24, 32, 48, 64, 128, 256];
put('electron/assets/appIcon.ico', await pngToIco(await Promise.all(icoSizes.map((size) => icon(size)))));

console.log('native assets: icons and splash regenerated from assets/icon.svg');
