# Bundles

A bundle is a directory or ZIP with `manifest.json`, a detached ed25519 signature `manifest.json.sig` and content-addressed `<sha256>.blob` assets. The installer fetches the newest one from the revola releases and verifies it before anything reaches the iPod.

## Manifest, schema 3

| Field | Meaning |
|---|---|
| `schema` | 3 |
| `purpose`, `version`, `minimum_installer_version` | as named; older installers refuse newer bundles |
| `compatibility` | target, hardware versions, BootROM hash |
| `core` | `api` (the `rv_api` major), `firmware_build_id`, `catalog_fingerprint` |
| `components` | `usb_helper`, `firmware`, `companion`, `nor`, each with a `format` and named files; a release bundle has all four, and its `nor` carries the `uninstaller` as well as the `image` and `descriptor` |
| `extensions` | id to `{format, version, api, file}`; `file` is the SHA-256 of the extension's one `.rvx` |
| `themes` | id to `{format, version, file}`; `format` is `revola-theme-v1`, `version` the theme's own string, `file` the SHA-256 of the `.rvt` |
| `assets` | hash to size |

`extensions` and `themes` are optional maps; either needs `core`.

`core.firmware_build_id` is the first eight hex digits of the pinned decrypted OSOS SHA-256 (`0x660eb6ff` for 2.0.5) and must match `firmware/apple.json`.

## Extension checks

Extension ids are lower-case ASCII, at most 32 characters. FAT32 is case-insensitive and may return a short name in upper case, so nothing depends on the case of a file name.

Each `.rvx` (revola `core/loader/rvx.h`, format 1) is checked as the core's loader checks it:

| Part | Check |
|---|---|
| header | carries `core.api` as its API major and the firmware build id |
| manifest, version 3 | carries the package id and the same API major, with `main` first |
| catalog | fits the file, lists the manifest's modules first and then the assets, word-aligned payloads, module sizes that add up |

The installer uploads each file to `/Revola/Extensions/<id>.rvx` after `/Revola/Firmware/revola.bin`; the helper creates missing directories.

## Theme checks

Theme ids are `[a-z0-9][a-z0-9_-]{0,30}`, the file's stem; `classic` is the built-in look and never a file. Each `.rvt` (`themes/rvt/data/rvt.h`, version 2) is checked as the Appearance extension's reader checks the header before it opens a pack:

| Part | Check |
|---|---|
| header | magic `RVT0`, version 2, header bytes a multiple of 4 from 128 up to the file size, CRC-32 of the header with its CRC word as zero, catalog fingerprint equal to `core.catalog_fingerprint` |
| appearances | at least one of light and dark, no other bits; an absent pack has offset and size zero |
| packs | word-aligned, at or after the header, at least a pack header and style prefix long (872 bytes), inside the file |
| id, name | NUL-terminated with zeros after; the id equals the map key, the name is printable UTF-8 |

The pack bodies are the iPod's to check.

The installer uploads each theme to `/Revola/Themes/<id>.rvt` after the extensions; the helper creates the folder. Uninstalling removes the whole `/Revola` tree, themes copied there by hand included, as it does for extensions.

## Building a bundle

`revola bundle export --spec FILE --base DIR --out DIR` builds a bundle from a specification with the same fields minus `schema` and `assets`, with file paths instead of hashes. `sign`, `pack`, `inspect`, `fetch` and `assemble` operate on the result. `tools/bundle.py` drives export and pack through the CLI for a release.
