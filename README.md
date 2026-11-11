# installer

Desktop installer for RevolaOS and the Rust crates it is built on: device access over DFU, firmware input preparation, recipe assembly and bundle verification, plus the command-line tool and the on-device USB helper.

## Layout

| Path | Contents |
|---|---|
| `crates/revola-device` | BootROM DFU checks, NOR backup, AES decryption through the device, verified file uploads and removal, bootloader installation |
| `crates/revola-bundle` | Signed release bundles, IPSW and AUPD preparation, recipe assembly |
| `crates/revola-cli` | The `revola` command line tool over both crates |
| `firmware/apple.json` | Sizes and hashes of the pinned Apple 2.0.5 inputs; no Apple bytes |
| `usb-helper/` | On-device upload helper: streams a file to the FAT32 volume, verifies readback or removes the RevolaOS files, returns to DFU |
| `rockbox/` | Patch applied to the pinned Rockbox checkout the helper is built from |
| `tools/` | `setup.py` vendors Rockbox, `firmware.py` reads the profile, `bundle.py` exports and packs bundles through the CLI |

## Crates and CLI

Requires Rust, libusb and pkg-config.

```sh
cargo build --release -p revola-cli
target/release/revola --help
target/release/revola devices
target/release/revola check
target/release/revola nor-dump --output nor.bin
target/release/revola firmware inspect --ipsw iPod_38.2.0.5.ipsw
target/release/revola bundle apply-recipe --recipe osos.json --data osos.data --inputs DECRYPTED_DIR --out osos-cfw.bin
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Installation and BootROM inspection commands use BootROM DFU; select an iPod with `--device BUS:ADDRESS` and add `--json` for machine-readable output. `apply-recipe` applies a schema 2 recipe (`classic7g-file-v1`) to decrypted Apple inputs on the user's machine; the recipe builder in the `revola` repository produces the recipe and data files. Bundle components keep the schema 2 format names from RepriseOS so existing recipes assemble unchanged.

### USB debugging

`revola debug --help` lists commands for normal-mode devices running the USB Debug extension. Enable it in Settings > Extensions. While enabled, USB stays in the running UI instead of disk mode; disable it and reconnect the cable to sync. Build the extension with the SDK matching the installed core. The wire protocol is documented in the extensions repository's `usb-debug/README.md`.

`scroll` injects wheel movements, not exact row counts. It synchronizes against Apple's wheel state and refuses firmware other than the pinned image. Screen parsing and `goto` describe and navigate the current UI; they cannot make a screen check and later input atomic. In particular, scrolling Now Playing changes volume.

`time-site` attempts to stop timing after success or failure. If the stop is not acknowledged, the command fails and warns that timing may still be active.

The wheel addresses and offsets in `crates/revola-device/src/debug/symbols.rs` are generated from the canonical firmware header. From the workspace root, verify them with:

```sh
python3 installer/tools/debug_symbols.py --check revola/firmware/osos-2.0.5.h
```

Omit `--check` to regenerate after the firmware symbols have been verified and copied from reversing. Do not edit the Rust constants by hand.

### Bundles

A bundle is a directory or ZIP with `manifest.json`, a detached ed25519 signature `manifest.json.sig` and content-addressed `<sha256>.blob` assets. Schema 3:

| Field | Meaning |
|---|---|
| `schema` | 3 |
| `purpose`, `version`, `minimum_installer_version` | as named; older installers refuse newer bundles |
| `compatibility` | target, hardware versions, BootROM hash |
| `core` | `api` (the `rv_api` major) and `firmware_build_id` of the pinned OSOS |
| `components` | `usb_helper`, `osos`, `companion`, `nor`, each with a `format` and named files |
| `extensions` | id to `{format, version, api, file}`; `file` is the SHA-256 of the extension's one `.rvx` |
| `assets` | hash to size |

Extension ids are lower-case ASCII, at most 32 characters; FAT32 is case-insensitive and may return a short name in upper case, so nothing may depend on the case of a file name. `core.firmware_build_id` is the first eight hex digits of the pinned decrypted OSOS SHA-256 (`0x660eb6ff` for 2.0.5) and must match `firmware/apple.json`. Each `.rvx` (revola `core/loader/rvx.h`, format 1) is checked as the core's loader checks it: the header carries `core.api` as its API major and the firmware build id; the manifest (version 2) carries the package id and the same API major, with `main` first; the catalog fits the file, lists the manifest's modules first and then the assets, with word-aligned payloads inside the file and module sizes that add up. The installer uploads each file to `/Revola/Extensions/<id>.rvx` after `osos-cfw.bin`; the helper creates missing directories.

`revola bundle export --spec FILE --base DIR --out DIR` builds a bundle from a specification with the same fields minus `schema` and `assets`, with file paths instead of hashes; `sign`, `pack`, `inspect`, `fetch` and `assemble` operate on the result.

Offline replay tests are ignored unless these variables point at preserved 2.0.5 material outside the repository: `REVOLA_ASSEMBLY_ROOT`, `REVOLA_ASSEMBLY_BUILD`, `REVOLA_TEST_IPSW`, `REVOLA_TEST_PACKAGE`, `REVOLA_TEST_BOOTROM` and optionally `REVOLA_TEST_NOR`.

DFU code is adapted from wInd3x; the PIPE0 protocol follows Rockbox's `ipoddfu.c`. See NOTICE.

## USB upload helper

The helper is a Rockbox-based image sent over BootROM DFU. It enumerates as WinUSB device `f055:180d`, writes one file to the data partition and reads it back, or deletes `/osos-cfw.bin`, `/cfw-loader.bin`, `/Revola/` and unfinished uploads, then returns to DFU. Building it needs GNU make, `arm-elf-eabi-gcc` 9.5.0 (the version is pinned in `firmware/apple.json`) and the Rockbox checkout pinned in `dependencies.lock`:

```sh
python3 tools/setup.py            # clones Rockbox into vendor/ and applies rockbox/*.patch
python3 tools/setup.py --check    # verifies the pin
python3 usb-helper/build.py --toolchain /path/to/bin --out build/usb-helper
python3 -m unittest discover -s usb-helper/tests -p '*_test.py'
```

`build/usb-helper/upload.dfu` is the IMG1 image and `manifest.json` describes it (schema 3: BootROM hash, size, SHA-256, offsets of the nonce, config and cold-init slots, and the `storage_inspection` and `file_removal` capabilities). Both go into a bundle as the `usb_helper` component. `usb-helper/tests/fixtures/` holds one built image for the Rust and Python tests; it contains Rockbox and helper code only.

## Desktop app

`app/` holds the Tauri 2 and React installer. It is a member of the root Cargo workspace and uses the crates by path, so one `cargo test --workspace` covers it. Requires Node.js, pnpm and the Tauri platform prerequisites besides the Rust toolchain.

```sh
cd app
pnpm install
pnpm tauri dev        # development build with hot reload
pnpm check            # typecheck, biome lint and frontend tests
pnpm tauri build      # bundles under ../target/release/bundle
```

Install flow: disclaimer, IPSW selection, USB permissions on Linux, DFU detection, device checks, NOR backup, firmware assembly, upload and bootloader install.

Uninstall flow: package download (no IPSW), DFU detection, device checks, NOR backup, file removal, then the Rockbox dual-boot uninstaller, which moves Apple's bootloader back into place. Files go first because the RevolaOS bootloader stops when they are missing. The uninstaller is the optional `uninstaller` file of the bundle's `nor` component, built by `mks5lboot --mkdfu-uninst ipod6g`.

Backups and assembled firmware are written under the app data directory for `org.revolaos.installer`. The About screen shows the NOTICE text and the version.

Release bundles are fetched from the newest published release of `RevolaOS/revola` (`<target>-manifest.json` asset) and verified with the ed25519 key in `app/src-tauri/src/package.rs`; `REVOLA_BUNDLE_MANIFEST_URL` and `REVOLA_BUNDLE_PUBLIC_KEY` override both at build time. Installer updates come from `RevolaOS/installer` releases through the Tauri updater; the matching minisign public key is in `tauri.conf.json`. The release notes shown for a firmware package and for an installer update are the bodies of those GitHub releases, read when the installer checks.

Icons derive from `app/src-tauri/icons/source/*.svg`. To replace them, render a new 1024 px `icons/source.png` and run `pnpm tauri icon src-tauri/icons/source.png` from `app/`.

TODO: Icon Composer `.icon` bundle for macOS 26 layered icons.

## Credits

RevolaOS started from the groundwork of [RepriseOS](https://github.com/lgreitis/osos-lab) by lgreitis: the boot chain, the installer's device and bundle code, and the reverse engineering of Apple's 2.0.5 firmware. You can support lgreitis on [Ko-fi](https://ko-fi.com/lgreitis).

## License

GPL-3.0-only. See COPYING and NOTICE.
