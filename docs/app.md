# Desktop app

`app/` holds the Tauri 2 and React installer. It is a member of the root Cargo workspace and uses the crates by path, so one `cargo test --workspace` covers it.

## Build

Requires Node.js, pnpm and the Tauri platform prerequisites besides the Rust toolchain.

```sh
cd app
pnpm install
pnpm tauri dev        # development build with hot reload
pnpm check            # typecheck, biome lint and frontend tests
pnpm tauri build      # bundles under ../target/release/bundle
```

## Install flow

1. Disclaimer
2. Firmware: the IPSW file and the RevolaOS package
3. USB permissions, on Linux
4. DFU detection
5. Device checks
6. Summary
7. NOR backup, firmware assembly, upload, bootloader install
8. Restart

The installer keeps the installed bootloader only when its code hash, size and entry point exactly match the package. A different custom bootloader is replaced after a fresh NOR backup.

## Uninstall flow

1. Package download, no IPSW
2. DFU detection
3. Device checks
4. NOR backup
5. File removal
6. Rockbox dual-boot uninstaller, which moves Apple's bootloader back into place

Files go first because the RevolaOS bootloader stops when they are missing. The uninstaller is the optional `uninstaller` file of the bundle's `nor` component, built by `mks5lboot --mkdfu-uninst ipod6g`.

## Data

Backups and assembled firmware are written under the app data directory for `org.revolaos.installer`. The About screen shows the NOTICE text and the version.

## Releases and updates

| What | Source | Verified with |
|---|---|---|
| Firmware bundle | newest published release of `RevolaOS/revola`, `<target>-manifest.json` asset | ed25519 key in `app/src-tauri/src/package.rs` |
| Installer update | `RevolaOS/installer` releases through the Tauri updater | minisign key in `tauri.conf.json` |

`REVOLA_BUNDLE_MANIFEST_URL` and `REVOLA_BUNDLE_PUBLIC_KEY` override the bundle source at build time. The release notes shown for a firmware package and for an installer update are the bodies of those GitHub releases, read when the installer checks.

## Icons

Icons derive from `app/src-tauri/icons/source/*.svg`. To replace them, render a new 1024 px `icons/source.png` and run `pnpm tauri icon src-tauri/icons/source.png` from `app/`.

TODO: Icon Composer `.icon` bundle for macOS 26 layered icons.
