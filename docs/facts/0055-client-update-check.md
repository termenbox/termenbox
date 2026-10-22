# FACT-0055: Client update check uses alias endpoints

**Date:** 2026-10-22
**Status:** accepted

## Summary

The client's server-release check (`packages/client/src/lib/update.ts`) queries GitHub's alias releases instead of the paginated releases list, avoiding the "nightly floods the first page" problem.

## Endpoints per channel

| Channel | Endpoint | Notes |
|---|---|---|
| stable | `GET /repos/termenbox/termenbox/releases/latest` | Returns latest non-prerelease; no pagination. |
| beta | `GET /repos/termenbox/termenbox/releases/tags/channel-beta` | Returns alias release; version parsed from `body` or `name`. |
| nightly | `GET /repos/termenbox/termenbox/releases/tags/channel-nightly` | Returns alias release; version parsed from `body` or `name`. |

## Alias release format

The alias release's `body` (or `name`) contains a machine-readable line:

```
version=0.2.0-beta.2
tag=v0.2.0-beta.2
published_at=2026-10-22T12:34:56Z
url=https://github.com/termenbox/termenbox/releases/tag/v0.2.0-beta.2
```

The client parses this (simple line-by-line `key=value`). The `url` field is the versioned release's web URL (for "View release" link).

## Caching and rate limits

- Cache TTL: 6 hours (per channel, per origin, in `localStorage`).
- Conditional requests: `ETag` from response stored; subsequent requests send `If-None-Match`. 304 responses don't count against unauthenticated limit (60 req/hour/IP).
- Exponential backoff on 403/429.
- User-Agent: `termenbox-client/{version}`.

## UI separation

- **PWA bundle update** (`updateAvailable` store): triggers reload button. Checked via existing `checkForUpdate()` (compares running bundle hash with `/index.html`).
- **Server release available**: shows a toast/badge with version and "View release" link. **No reload button**. If channel differs from build channel, shows "Open {channel} app" link instead.

## Related

- DECISION-0125 (release channels)
- DECISION-0128 (three Pages deployments)
- `packages/client/src/lib/update.ts`, `packages/client/src/lib/settings.ts`, `packages/client/src/App.svelte`