# termenbox-native

Optional native wrappers for the Termenbox client. The PWA in `packages/client`
stays the reference client; this workspace loads the same built client and never
becomes a requirement for it.

## Verified stack

- `@capacitor/core` **8.5.3**
- `@capacitor/cli` **8.5.3**
- `@capacitor-community/electron` **5.0.1**

The community Electron platform was verified with the Capacitor 8 CLI before it
was pinned: `npx cap add @capacitor-community/electron` loaded the platform and
generated an Electron project. R76 records the command output as evidence.

## Scope

This workspace currently holds the pinned wrapper toolchain only. The platform
projects (iOS, Android, Electron), the capability adapter and the serving model
are later slices of R76; no `webDir` or serving choice is made here.

See `docs/plans/roadmap/R76.md` and
`docs/decisions/0140-native-wrappers-are-optional-and-the-pwa-stays-first-class.md`.
