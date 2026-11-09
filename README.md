# installer

Desktop installer for RevolaOS and the Rust crates it is built on: device access over DFU, firmware input preparation, recipe assembly and bundle verification, plus the command-line tool and the on-device USB helper.

## Layout

| Path | Contents |
|---|---|
| `crates/revola-device` | BootROM DFU checks, NOR backup, AES decryption through the device, verified file uploads, bootloader installation |
| `crates/revola-bundle` | Signed release bundles, IPSW and AUPD preparation, recipe assembly |
| `crates/revola-cli` | The `revola` command line tool over both crates |
| `firmware/apple.json` | Sizes and hashes of the pinned Apple 2.0.5 inputs; no Apple bytes |
| `usb-helper/` | On-device upload helper: streams a file to the FAT32 volume, verifies readback, returns to DFU |
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

Device commands use BootROM DFU; select an iPod with `--device BUS:ADDRESS` and add `--json` for machine-readable output. `apply-recipe` applies a schema 2 recipe (`classic7g-file-v1`) to decrypted Apple inputs on the user's machine; the recipe builder in the `revola` repository produces the recipe and data files. Bundle components keep the schema 2 format names from RepriseOS so existing recipes assemble unchanged.

### Bundles

A bundle is a directory or ZIP with `manifest.json`, a detached ed25519 signature `manifest.json.sig` and content-addressed `<sha256>.blob` assets. Schema 3:

| Field | Meaning |
|---|---|
| `schema` | 3 |
| `purpose`, `version`, `minimum_installer_version` | as named; older installers refuse newer bundles |
| `compatibility` | target, hardware versions, BootROM hash |
| `core` | `api` (the `rv_api` major) and `firmware_build_id` of the pinned OSOS |
| `components` | `usb_helper`, `osos`, `companion`, `nor`, each with a `format` and named files |
| `extensions` | id to `{format, version, api, files}`; files are `manifest.bin` plus `.rvx` modules |
| `assets` | hash to size |

Extension ids are lower-case ASCII, at most 32 characters; FAT32 is case-insensitive and may return a short name in upper case, so nothing may depend on the case of a directory name. `core.firmware_build_id` is the first eight hex digits of the pinned decrypted OSOS SHA-256 (`0x660eb6ff` for 2.0.5) and must match `firmware/apple.json`. Each extension's `manifest.bin` (revola `tools/manifest.py` layout, version 1) must carry the package id, an API major equal to `core.api`, and a module list whose `<name>.rvx` files are exactly the other files in the package, `main` first. Each `.rvx` header (format 1) must carry that API major, the firmware build id, the FNV-1a hash of `<id>/<name>` and sizes that add up to the file length. The installer uploads the files to `/Revola/Extensions/<id>/` after `osos-cfw.bin`; the helper creates missing directories.

`revola bundle export --spec FILE --base DIR --out DIR` builds a bundle from a specification with the same fields minus `schema` and `assets`, with file paths instead of hashes; `sign`, `pack`, `inspect`, `fetch` and `assemble` operate on the result.

Offline replay tests are ignored unless these variables point at preserved 2.0.5 material outside the repository: `REVOLA_ASSEMBLY_ROOT`, `REVOLA_ASSEMBLY_BUILD`, `REVOLA_TEST_IPSW`, `REVOLA_TEST_PACKAGE`, `REVOLA_TEST_BOOTROM` and optionally `REVOLA_TEST_NOR`.

DFU code is adapted from wInd3x; the PIPE0 protocol follows Rockbox's `ipoddfu.c`. See NOTICE.

## USB upload helper

The helper is a Rockbox-based image sent over BootROM DFU. It enumerates as WinUSB device `f055:180d`, writes one file to the data partition, reads it back and returns to DFU. Building it needs GNU make, `arm-elf-eabi-gcc` 9.5.0 (the version is pinned in `firmware/apple.json`) and the Rockbox checkout pinned in `dependencies.lock`:

```sh
python3 tools/setup.py            # clones Rockbox into vendor/ and applies rockbox/*.patch
python3 tools/setup.py --check    # verifies the pin
python3 usb-helper/build.py --toolchain /path/to/bin --out build/usb-helper
python3 -m unittest discover -s usb-helper/tests -p '*_test.py'
```

`build/usb-helper/upload.dfu` is the IMG1 image and `manifest.json` describes it (schema 3: BootROM hash, size, SHA-256, offsets of the nonce, config and cold-init slots). Both go into a bundle as the `usb_helper` component. `usb-helper/tests/fixtures/` holds one built image for the Rust and Python tests; it contains Rockbox and helper code only.

## Desktop app

`app/` holds the Tauri 2 and React installer. It is a member of the root Cargo workspace and uses the crates by path, so one `cargo test --workspace` covers it. Requires Node.js, pnpm and the Tauri platform prerequisites besides the Rust toolchain.

```sh
cd app
pnpm install
pnpm tauri dev        # development build with hot reload
pnpm check            # typecheck, biome lint and frontend tests
pnpm tauri build      # bundles under ../target/release/bundle
```

Install flow: disclaimer, IPSW selection, USB permissions on Linux, DFU detection, device checks, NOR backup, firmware assembly, upload and bootloader install. Backups and assembled firmware are written under the app data directory for `org.revolaos.installer`. The About screen shows the NOTICE text and the version.

Release bundles are fetched from the newest published release of `RevolaOS/revola` (`<target>-manifest.json` asset) and verified with the ed25519 key in `app/src-tauri/src/package.rs`; `REVOLA_BUNDLE_MANIFEST_URL` and `REVOLA_BUNDLE_PUBLIC_KEY` override both at build time. Installer updates come from `RevolaOS/installer` releases through the Tauri updater; the matching minisign public key is in `tauri.conf.json`.

Icons derive from `app/src-tauri/icons/source/*.svg`. To replace them, render a new 1024 px `icons/source.png` and run `pnpm tauri icon src-tauri/icons/source.png` from `app/`.

TODO: Icon Composer `.icon` bundle for macOS 26 layered icons.

## What drives development

1. **RAM first.** 64 MiB in total, about 20 MiB of heap shared with Apple's music database. Every pull request reports resident and peak bytes from the emulator heap profile. An increase needs a stated reason. A disabled extension costs zero bytes. Reuse what exists before adding anything.
2. **Performance second.** No work in audio callbacks beyond the DSP itself. No redraw of unchanged pixels. No disk reads for things that can be computed. No per-draw checks when a one-time hook can decide. Hot paths bind directly; the dispatcher runs only when two handlers share a site.
3. **Optimise before merge.** Measure, remove what measurement does not justify, and stop when the next step costs more clarity than it saves. "Works" is not the bar.
4. **Complexity matches need.** No abstraction for one caller, no layer that only forwards. Defer anything with one user, delete anything with none. A person must be able to reason about the code.
5. **Clean structure.** One concern per directory. Extensions never reach into core internals. Apple addresses come only from the generated symbols header.
6. **Measure, do not guess.** Emulator heap profile for every RAM claim; hardware for audio, cache and storage behaviour.

## Core or extension

Something is core only if the loader, SDK or settings store needs it, if it must act before the loader can run, or if the boot cannot recover without it. Everything else is an extension, however small. One extension per concern. If one feature cannot work without another, it is a module of that extension, not a dependency.


## License

GPL-3.0-only. See COPYING and NOTICE.
