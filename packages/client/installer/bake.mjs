// The installer is served at /i with the address of the client that serves it as its default, so a self-hosted client
// shows the same bare command as the hosted one. The line to change carries the marker `# client-origin`.
const MARK = /^(ORIGIN="\$\{TERMENBOX_CLIENT_URL:-)[^}]*(\}" # client-origin)/m;
const ORIGIN = /^(https:\/\/[A-Za-z0-9.-]+(:\d+)?|http:\/\/(localhost|127\.0\.0\.1)(:\d+)?)$/;

export function bakeInstaller(source, origin) {
  if (!origin) return source;
  const o = origin.replace(/\/$/, '');
  if (!ORIGIN.test(o)) throw new Error(`TERMENBOX_CLIENT_ORIGIN must be an https:// address with no path, got "${origin}"`);
  if (!MARK.test(source)) throw new Error('install.sh has lost its "# client-origin" line');
  return source.replace(MARK, `$1${o}$2`);
}
