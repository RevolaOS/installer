# Bundles

A bundle is a directory or ZIP with `manifest.json`, a detached ed25519 signature `manifest.json.sig` and content-addressed `<sha256>.blob` assets. The installer fetches the newest one from the revola releases and verifies it before anything reaches the iPod.

## Manifest, schema 3

| Field | Meaning |
|---|---|
| `schema` | 3 |
| `purpose`, `version`, `minimum_installer_version` | as named; older installers refuse newer bundles |
| `compatibility` | target, hardware versions, BootROM hash |
| `core` | `api` (the `rv_api` major) and `firmware_build_id` of the pinned OSOS |
| `components` | `usb_helper`, `firmware`, `companion`, `nor`, each with a `format` and named files |
| `extensions` | id to `{format, version, api, file}`; `file` is the SHA-256 of the extension's one `.rvx` |
| `assets` | hash to size |

`core.firmware_build_id` is the first eight hex digits of the pinned decrypted OSOS SHA-256 (`0x660eb6ff` for 2.0.5) and must match `firmware/apple.json`.

## Extension checks

Extension ids are lower-case ASCII, at most 32 characters. FAT32 is case-insensitive and may return a short name in upper case, so nothing depends on the case of a file name.

Each `.rvx` (revola `core/loader/rvx.h`, format 1) is checked as the core's loader checks it:

| Part | Check |
|---|---|
| header | carries `core.api` as its API major and the firmware build id |
| manifest, version 2 | carries the package id and the same API major, with `main` first |
| catalog | fits the file, lists the manifest's modules first and then the assets, word-aligned payloads, module sizes that add up |

The installer uploads each file to `/Revola/Extensions/<id>.rvx` after `/Revola/Firmware/revola.bin`; the helper creates missing directories.

## Building a bundle

`revola bundle export --spec FILE --base DIR --out DIR` builds a bundle from a specification with the same fields minus `schema` and `assets`, with file paths instead of hashes. `sign`, `pack`, `inspect`, `fetch` and `assemble` operate on the result. `tools/bundle.py` drives export and pack through the CLI for a release.
