// The extension registry, the TypeScript side of packages/core/src/ext.rs (decision 0136, fact 0057).
// Extensions ride in `_meta` under `termenbox.dev/<name>` or as methods `_termenbox.dev/<name>`; both sides list what
// they implement at `initialize` and only what both list is used. Shared vectors: packages/core/testdata/extension-vectors.json.

export const PROTOCOL_VERSION = 1;
export const NAMESPACE = 'termenbox.dev';
export const METHOD_NOT_FOUND = -32601;
const KEY_PREFIX = 'termenbox.dev/';
const METHOD_PREFIX = '_termenbox.dev/';

export type Meta = Record<string, unknown>;

export const key = (name: string) => `${KEY_PREFIX}${name}`;
export const method = (name: string) => `${METHOD_PREFIX}${name}`;

/** What the other side and this side both implement, at the lower version. `protocol` is 0 for a plain ACP peer. */
export class Negotiated {
  constructor(readonly protocol = 0, private readonly versions: ReadonlyMap<string, number> = new Map()) {}
  version(name: string): number | undefined { return this.versions.get(name); }
  supports(name: string): boolean { return this.versions.has(name); }
  toJSON() { return { protocol: this.protocol, versions: Object.fromEntries(this.versions) }; }
}

const uint = (v: unknown): number | undefined => (typeof v === 'number' && Number.isInteger(v) && v >= 0 ? v : undefined);
const isObject = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

/** What this side implements: extension name -> highest version. */
export class Registry {
  constructor(private readonly versions: Record<string, number> = {}) {}

  with(name: string, version: number): Registry { return new Registry({ ...this.versions, [name]: version }); }

  offer() { return { protocol: PROTOCOL_VERSION, extensions: { ...this.versions } }; }

  advertise(capabilityMeta: Meta) { capabilityMeta[NAMESPACE] = this.offer(); }

  /** A missing or unreadable offer is a plain ACP peer; an unreadable entry is skipped alone. */
  negotiate(remoteCapabilityMeta?: Meta | null): Negotiated {
    const remote = remoteCapabilityMeta?.[NAMESPACE];
    const protocol = isObject(remote) ? uint(remote.protocol) : undefined;
    if (!isObject(remote) || protocol === undefined) return new Negotiated();
    const versions = new Map<string, number>();
    const theirs = remote.extensions;
    if (isObject(theirs)) {
      for (const [name, ours] of Object.entries(this.versions)) {
        const v = uint(theirs[name]);
        if (v !== undefined) versions.set(name, Math.min(v, ours));
      }
    }
    return new Negotiated(Math.min(protocol, PROTOCOL_VERSION), versions);
  }
}

/** Removes every extension key the peer did not negotiate. The offer itself and other namespaces stay. */
export function stripUnnegotiated(meta: Meta, negotiated: Negotiated): Meta {
  return Object.fromEntries(Object.entries(meta).filter(([k]) => !k.startsWith(KEY_PREFIX) || negotiated.supports(k.slice(KEY_PREFIX.length))));
}

/** Removes every key of our namespace: what is left must be valid plain ACP (guardrail protocol-core-stays-acp-v1). */
export function stripAll(meta: Meta): Meta {
  return Object.fromEntries(Object.entries(meta).filter(([k]) => !k.startsWith(KEY_PREFIX) && k !== NAMESPACE));
}

/** The extension `name` if negotiated and sent; `undefined` when skipped. The caller validates the shape. */
export function getExtension(meta: Meta | undefined, name: string, negotiated: Negotiated): unknown {
  return negotiated.supports(name) ? meta?.[key(name)] : undefined;
}

/** Adds `value` under the extension's key if the peer negotiated it; returns whether it was written. */
export function putExtension(meta: Meta, name: string, value: unknown, negotiated: Negotiated): boolean {
  if (!negotiated.supports(name)) return false;
  meta[key(name)] = value;
  return true;
}

export type Route = { kind: 'core' } | { kind: 'extension'; name: string } | { kind: 'reject' } | { kind: 'skip' };

/** An unknown extension request is answered with METHOD_NOT_FOUND, an unknown notification is ignored. */
export function route(methodName: string, isRequest: boolean, negotiated: Negotiated): Route {
  if (!methodName.startsWith('_')) return { kind: 'core' };
  if (methodName.startsWith(METHOD_PREFIX)) {
    const name = methodName.slice(METHOD_PREFIX.length);
    if (negotiated.supports(name)) return { kind: 'extension', name };
  }
  return { kind: isRequest ? 'reject' : 'skip' };
}
