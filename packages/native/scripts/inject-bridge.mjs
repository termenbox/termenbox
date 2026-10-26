import { copyFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The web dir Capacitor copies the client build into for each mobile platform.
const WEB_DIR = {
  android: 'android/app/src/main/assets/public',
  ios: 'ios/App/App/public',
};

const targets = process.argv.slice(2);
if (targets.length === 0) {
  console.error('usage: inject-bridge.mjs <android|ios> [...]');
  process.exit(2);
}

for (const target of targets) {
  const dir = WEB_DIR[target];
  if (!dir) {
    console.error(`unknown target: ${target}`);
    process.exit(2);
  }
  const index = join(dir, 'index.html');
  if (!existsSync(index)) {
    console.error(`missing ${index}; run \`cap sync ${target}\` (or \`cap copy ${target}\`) first`);
    process.exit(1);
  }
  copyFileSync('bridge/dist/bridge.js', join(dir, 'bridge.js'));
  const html = readFileSync(index, 'utf8');
  if (!html.includes('src="bridge.js"')) {
    writeFileSync(index, html.replace('</head>', '    <script src="bridge.js"></script>\n  </head>'));
  }
  console.log(`bridge injected: ${dir}`);
}
