// The installer is served at /i with the address of the client that serves it as its default, so a self-hosted client
// shows the same bare command as the hosted one. The line to change carries the marker `# client-origin`.
// Each channel's deployment also bakes its channel (`# default-channel`) and the release keys that channel trusts
// (`# release-keys`), so the beta app's installer follows beta and cannot be made to accept a nightly key (decision 0134).
const MARK = /^(ORIGIN="\$\{TERMENBOX_CLIENT_URL:-)[^}]*(\}" # client-origin)/m;
const CHANNEL_MARK = /^(DEFAULT_CHANNEL=")[a-z]*(" # default-channel)/m;
const KEYS_MARK = /^(RELEASE_KEYS="\$\{TERMENBOX_RELEASE_KEYS:-)[^}]*(\}" # release-keys)/m;
const ORIGIN = /^(https:\/\/[A-Za-z0-9.-]+(:\d+)?|http:\/\/(localhost|127\.0\.0\.1)(:\d+)?)$/;

export const CHANNELS = ['stable', 'beta', 'nightly'];

// The keys a channel's installer trusts, from release-keys.txt. A key without `channel=` is a stable key. Stable trusts
// stable keys; beta trusts beta and stable keys (the beta alias can hold a stable release); nightly trusts nightly keys.
export function channelKeys(keysText, channel) {
  const follows = { stable: ['stable'], beta: ['beta', 'stable'], nightly: ['nightly'] }[channel];
  if (!follows) throw new Error(`channel must be one of ${CHANNELS.join(', ')}, got "${channel}"`);
  const lines = keysText
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith('#'))
    .filter((l) => follows.includes(/\bchannel=(\w+)$/.exec(l)?.[1] ?? 'stable'));
  if (!lines.length) throw new Error(`release-keys.txt has no key for the ${channel} channel`);
  // The installer holds the list on one line inside double quotes, so it is joined with real newlines kept out of the
  // source: ssh keys are base64 and `+` and `/` are safe there, the characters that are not are refused.
  for (const l of lines) if (!/^[A-Za-z0-9._-]+ ssh-ed25519 [A-Za-z0-9+/=]+( channel=[a-z]+)?$/.test(l)) throw new Error(`unexpected key line "${l}"`);
  return lines;
}

export function bakeInstaller(source, origin, channel, keysText) {
  let out = source;
  if (origin) {
    const o = origin.replace(/\/$/, '');
    if (!ORIGIN.test(o)) throw new Error(`TERMENBOX_CLIENT_ORIGIN must be an https:// address with no path, got "${origin}"`);
    if (!MARK.test(out)) throw new Error('install.sh has lost its "# client-origin" line');
    out = out.replace(MARK, `$1${o}$2`);
  }
  if (channel) {
    if (!CHANNELS.includes(channel)) throw new Error(`channel must be one of ${CHANNELS.join(', ')}, got "${channel}"`);
    if (!CHANNEL_MARK.test(out)) throw new Error('install.sh has lost its "# default-channel" line');
    out = out.replace(CHANNEL_MARK, `$1${channel}$2`);
    if (keysText !== undefined) {
      if (!KEYS_MARK.test(out)) throw new Error('install.sh has lost its "# release-keys" line');
      // A newline inside the quoted default is legal shell; a replacement function keeps `$` in the keys literal.
      out = out.replace(KEYS_MARK, (_m, a, b) => `${a}${channelKeys(keysText, channel).join('\n')}${b}`);
    }
  }
  return out;
}
