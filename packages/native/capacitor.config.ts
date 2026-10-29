import type { CapacitorConfig } from '@capacitor/cli';

const config: CapacitorConfig = {
  appId: 'dev.termenbox.app',
  appName: 'Termenbox',
  // Bundled assets: the wrapper ships the same build as the PWA and never loads
  // a remote origin. See packages/native/README.md for why.
  webDir: '../client/dist',
};

export default config;
