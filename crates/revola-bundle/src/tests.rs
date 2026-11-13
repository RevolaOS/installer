// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::le::word;

pub(crate) struct Fixture {
    pub dir: tempfile::TempDir,
    pub key: TrustedKey,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let image = include_bytes!("../../../usb-helper/tests/fixtures/upload.dfu");
        let descriptor = include_bytes!("../../../usb-helper/tests/fixtures/manifest.json");
        let image_hash = sha256(image);
        let descriptor_hash = sha256(descriptor);
        let manifest = serde_json::json!({
            "schema": SCHEMA, "purpose": "development", "version": "0.2.0",
            "minimum_installer_version": "0.2.0",
            "compatibility": revola_device::firmware::current().compatibility,
            "components": {"usb_helper": {"format": "revola-upload-v1", "files": {"image": image_hash, "descriptor": descriptor_hash}}},
            "assets": {image_hash.clone(): {"bytes": image.len()}, descriptor_hash.clone(): {"bytes": descriptor.len()}}
        });
        fs::write(
            dir.path().join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(dir.path().join(asset_filename(&image_hash)), image).unwrap();
        fs::write(
            dir.path().join(asset_filename(&descriptor_hash)),
            descriptor,
        )
        .unwrap();
        let public = sign_directory(dir.path(), &"17".repeat(32), "0.2.0").unwrap();
        Self {
            dir,
            key: TrustedKey::from_hex(&public).unwrap(),
        }
    }

    fn load(&self) -> Result<VerifiedBundle> {
        VerifiedBundle::load(self.dir.path(), &self.key, "0.2.0")
    }
}

#[test]
fn signature_and_device_compatibility() {
    let f = Fixture::new();
    let b = f.load().unwrap();
    let c = &b.manifest().compatibility;
    for hardware in [0x00130000, 0x00130100, 0x00130200, 0x00130300] {
        assert!(b.require_device(hardware, &c.bootrom_sha256).is_ok());
    }
    assert!(b.require_device(0x12340000, &c.bootrom_sha256).is_err());
    assert!(b.require_device(0, &c.bootrom_sha256).is_err());
    assert!(b.require_device(0x00130200, "wrong-rom").is_err());
    assert!(b.file("nor", "image").is_err());
    let wrong = SigningKey::from_bytes(&[4; 32]);
    let key = TrustedKey::from_hex(&hex(&wrong.verifying_key().to_bytes())).unwrap();
    assert!(VerifiedBundle::load(f.dir.path(), &key, "0.2.0").is_err());
    assert!(VerifiedBundle::load(f.dir.path(), &f.key, "0.1.9").is_err());
    assert!(sign_directory(f.dir.path(), &"17".repeat(32), "0.2.0").is_err());
}

#[test]
fn tampered_manifest_signature_and_assets_are_rejected() {
    for file in [MANIFEST_FILE, SIGNATURE_FILE, "asset"] {
        let f = Fixture::new();
        let name = if file == "asset" {
            asset_filename(f.load().unwrap().manifest().assets.keys().next().unwrap())
        } else {
            file.to_owned()
        };
        let path = f.dir.path().join(name);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(f.load().is_err(), "{file}");
    }
}

#[test]
fn release_manifest_needs_every_component_and_the_uninstaller() {
    let f = Fixture::new();
    let mut release = f.load().unwrap().manifest().clone();
    let hash = release.assets.keys().next().unwrap().clone();
    let pinned = &revola_device::firmware::current().inputs["osos.bin"].sha256;
    release.purpose = Purpose::Release;
    release.core = Some(Core {
        api: 1,
        firmware_build_id: extension::firmware_build_id(pinned).unwrap(),
        catalog_fingerprint: CATALOG_FINGERPRINT.into(),
    });
    for (name, format, files) in [
        (
            "firmware",
            "revola-firmware-recipe-v1",
            &["recipe", "data"][..],
        ),
        (
            "companion",
            "revola-companion-recipe-v1",
            &["recipe", "data"],
        ),
        ("nor", "revola-nor-template-v1", &["image", "descriptor"]),
    ] {
        release.components.insert(
            name.into(),
            Component {
                format: format.into(),
                files: files
                    .iter()
                    .map(|f| (f.to_string(), hash.clone()))
                    .collect(),
            },
        );
    }
    assert!(release.validate("0.2.0").is_err());
    release
        .components
        .get_mut("nor")
        .unwrap()
        .files
        .insert("uninstaller".into(), hash);
    assert!(release.validate("0.2.0").is_ok());
    // SemVer orders a prerelease before its release, so a beta installer needs a
    // beta minimum or it refuses the beta bundle.
    release.minimum_installer_version = "0.1.0".into();
    assert!(release.validate("0.1.0-beta.1").is_err());
    release.minimum_installer_version = "0.1.0-beta.1".into();
    assert!(release.validate("0.1.0-beta.1").is_ok());
    assert!(release.validate("0.1.0-beta.2").is_ok());
    assert!(release.validate("0.1.0").is_ok());
}

#[test]
fn manifest_contract_rejects_incomplete_releases_paths_unknown_formats_and_limits() {
    let f = Fixture::new();
    let original = f.load().unwrap().manifest().clone();
    let mut bad = original.clone();
    bad.purpose = Purpose::Release;
    assert!(bad.validate("0.2.0").is_err());
    let mut bad = original.clone();
    bad.components.get_mut("usb_helper").unwrap().format = "future".into();
    assert!(bad.validate("0.2.0").is_err());
    let mut bad = original.clone();
    bad.components
        .get_mut("usb_helper")
        .unwrap()
        .files
        .insert("../escape".into(), "../escape".into());
    assert!(bad.validate("0.2.0").is_err());
    let mut bad = original.clone();
    bad.assets.values_mut().next().unwrap().bytes = MAX_ASSET_BYTES + 1;
    assert!(bad.validate("0.2.0").is_err());
    let mut bad = original.clone();
    bad.assets.insert("0".repeat(64), Asset { bytes: 1 });
    assert!(bad.validate("0.2.0").is_err());
    for hardware in [vec![], vec![0], vec![0x00130200, 0x00130200]] {
        let mut bad = original.clone();
        bad.compatibility.hardware_versions = hardware;
        assert!(bad.validate("0.2.0").is_err());
    }
}

#[test]
fn future_packages_explain_installer_requirements() {
    let f = Fixture::new();
    let mut manifest = f.load().unwrap().manifest().clone();
    manifest.minimum_installer_version = "0.3.0".into();
    let expected = "This firmware release requires installer 0.3.0 or newer. Update the RevolaOS installer, then try again.";
    assert_eq!(
        manifest.validate("0.2.0").unwrap_err().to_string(),
        expected
    );
    assert!(manifest.validate("0.3.0").is_ok());
    assert!(manifest.validate("0.3.1").is_ok());

    manifest.schema = SCHEMA + 1;
    assert_eq!(
        manifest.validate("0.2.0").unwrap_err().to_string(),
        expected
    );
    assert_eq!(
        manifest.validate("0.3.0").unwrap_err().to_string(),
        "This firmware package uses an unsupported format. Update the RevolaOS installer, then try again."
    );
}

/// An extension file from revola's rvx-pack (tools/rvx.py `pack_extension`): id eq, API
/// 1.7, modules main (always) and editor (screen), a resident lz bitmap "icon" and a data
/// asset "table" read on request.
const REFERENCE_PACKAGE: &str = "525658300100000007000100ffb60e6674000000040000000100000045b6064c52564d460300000007000100000000000200000000000000380000000000030006000c001200170024000000000000002500000000002a000101310065710045510042616e647300302e312e30005061756c0047504c2d332e302d6f6e6c7900006d61696e00656469746f7200656469746f720008e290ea00000000e4000000240000002400000078820d5c00000000100100002000000020000000f01540e602010000080100000500000028000000df9b9c4a010000003001000006000000060000000800000004000000040000000100000001000000000102030405060708090a0b080000000211f0001500000008000000040000000000000000000000010000000000000000000000000000000001020304050000";
/// Offsets in REFERENCE_PACKAGE: the header's entry and resident counts, the catalog, and
/// the first module payload.
const ENTRY_COUNT: usize = 20;
const RESIDENT_COUNT: usize = 24;
const CATALOG: usize = 32 + 116;
const MAIN_PAYLOAD: usize = 0xe4;

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The themes catalog of the pinned firmware, `rvt/data/osos-2.0.5.json` in the themes
/// repository.
const CATALOG_FINGERPRINT: &str = "4f2e84ff031616d7";

fn core() -> Core {
    Core {
        api: 1,
        firmware_build_id: extension::firmware_build_id(
            &revola_device::firmware::current().inputs["osos.bin"].sha256,
        )
        .unwrap(),
        catalog_fingerprint: CATALOG_FINGERPRINT.into(),
    }
}

fn reference() -> Vec<u8> {
    unhex(REFERENCE_PACKAGE)
}

fn put(file: &mut [u8], at: usize, value: u32) {
    file[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn with_extension(
    f: &Fixture,
    id: &str,
    file: &[u8],
    api: u32,
) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let mut manifest: Manifest =
        serde_json::from_slice(&fs::read(f.dir.path().join(MANIFEST_FILE)).unwrap()).unwrap();
    manifest.core = Some(core());
    let hash = sha256(file);
    manifest
        .assets
        .insert(hash.clone(), Asset { bytes: file.len() });
    let assets = BTreeMap::from([(hash.clone(), file.to_vec())]);
    manifest.extensions.insert(
        id.to_owned(),
        Extension {
            format: EXTENSION_FORMAT.into(),
            version: "0.1.0".into(),
            api,
            file: hash,
        },
    );
    let referenced: std::collections::BTreeSet<String> = manifest
        .components
        .values()
        .flat_map(|c| c.files.values())
        .chain(manifest.extensions.values().map(|e| &e.file))
        .chain(manifest.themes.values().map(|t| &t.file))
        .cloned()
        .collect();
    manifest.assets.retain(|hash, _| referenced.contains(hash));
    (manifest, assets)
}

/// The fixture with one theme and no extension.
fn with_theme(f: &Fixture, id: &str, file: &[u8]) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let mut manifest: Manifest =
        serde_json::from_slice(&fs::read(f.dir.path().join(MANIFEST_FILE)).unwrap()).unwrap();
    manifest.extensions.clear();
    manifest.core = Some(core());
    let hash = sha256(file);
    manifest
        .assets
        .insert(hash.clone(), Asset { bytes: file.len() });
    manifest.themes.insert(
        id.to_owned(),
        Theme {
            format: THEME_FORMAT.into(),
            version: "1.0".into(),
            file: hash.clone(),
        },
    );
    let referenced: std::collections::BTreeSet<String> = manifest
        .components
        .values()
        .flat_map(|c| c.files.values())
        .chain(manifest.themes.values().map(|t| &t.file))
        .cloned()
        .collect();
    manifest.assets.retain(|hash, _| referenced.contains(hash));
    (manifest, BTreeMap::from([(hash, file.to_vec())]))
}

/// A theme file as the themes repository's `rvt` writes it: a 128-byte header, then a
/// pack of `PACK` bytes per appearance in `mask`, each a 16-byte pack header and a body
/// of the style prefix.
const PACK: usize = theme::MIN_PACK;
const APPEARANCES: usize = 16;
const LIGHT: usize = 20;
const DARK: usize = 28;
const HEADER_CRC: usize = 36;

fn theme(id: &str, name: &[u8], mask: u32) -> Vec<u8> {
    let mut file = vec![0; 128];
    file[..4].copy_from_slice(b"RVT0");
    file[4..6].copy_from_slice(&2u16.to_le_bytes());
    file[6..8].copy_from_slice(&128u16.to_le_bytes());
    file[8..16].copy_from_slice(&unhex(CATALOG_FINGERPRINT));
    put(&mut file, APPEARANCES, mask);
    let mut offset = 128;
    for (bit, at) in [(1, LIGHT), (2, DARK)] {
        if mask & bit != 0 {
            put(&mut file, at, offset as u32);
            put(&mut file, at + 4, PACK as u32);
            offset += PACK;
        }
    }
    file[40..40 + id.len()].copy_from_slice(id.as_bytes());
    file[72..72 + name.len()].copy_from_slice(name);
    let crc = crc32fast::hash(&file);
    put(&mut file, HEADER_CRC, crc);
    file.resize(offset, 0);
    file
}

fn write_signed(
    f: &Fixture,
    manifest: &Manifest,
    assets: &BTreeMap<String, Vec<u8>>,
) -> Result<String> {
    for (hash, bytes) in assets {
        fs::write(f.dir.path().join(asset_filename(hash)), bytes).unwrap();
    }
    fs::write(
        f.dir.path().join(MANIFEST_FILE),
        serde_json::to_vec_pretty(manifest).unwrap(),
    )
    .unwrap();
    let _ = fs::remove_file(f.dir.path().join(SIGNATURE_FILE));
    sign_directory(f.dir.path(), &"17".repeat(32), "0.2.0")
}

#[test]
fn extension_layouts_match_the_revola_tools() {
    assert!(extension::check_package("eq", &reference(), &core()).is_ok());
    assert_eq!(
        extension::firmware_build_id("660eb6ff5f49").unwrap(),
        0x660e_b6ff
    );
    assert_eq!(core().firmware_build_id, 0x660e_b6ff);
}

#[test]
fn extensions_are_validated_and_listed_in_upload_order() {
    let f = Fixture::new();
    let good = reference();
    let (manifest, assets) = with_extension(&f, "eq", &good, 1);
    write_signed(&f, &manifest, &assets).unwrap();
    let bundle = f.load().unwrap();
    let files: Vec<_> = bundle
        .extension_files()
        .into_iter()
        .map(|file| (file.id.to_owned(), file.destination))
        .collect();
    assert_eq!(
        files,
        [("eq".to_owned(), "/Revola/Extensions/eq.rvx".to_owned())]
    );
    assert_eq!(bundle.extension_bytes(), good.len());

    // Manifest-level rejections: id, api, missing core, other firmware.
    for (id, api) in [("EQ", 1), ("eq", 2)] {
        let (manifest, _) = with_extension(&f, id, &good, api);
        assert!(manifest.validate("0.2.0").is_err(), "{id} {api}");
    }
    let (mut manifest, _) = with_extension(&f, "eq", &good, 1);
    manifest.core = None;
    assert!(manifest.validate("0.2.0").is_err());
    let (mut manifest, _) = with_extension(&f, "eq", &good, 1);
    manifest.core.as_mut().unwrap().firmware_build_id ^= 1;
    assert!(manifest.validate("0.2.0").is_err());

    // Content-level rejections happen before signing and at load: the loader's checks.
    let edit = |at: usize, value: u32| {
        let mut file = good.clone();
        put(&mut file, at, value);
        file
    };
    let entry = |index: usize, field: usize| CATALOG + index * 20 + field;
    // One content folder naming the manifest string at `offset`: 0 "eq", 3 "EQ", 6 "Bands".
    let with_content = |file: &[u8], offset: u32| {
        let mut file = file.to_vec();
        put(&mut file, 32 + 40, 36 | offset << 16);
        put(&mut file, 32 + 44, 1);
        file
    };
    assert!(extension::check_package("eq", &with_content(&good, 6), &core()).is_ok());
    // One content folder named by a string appended to the manifest's string table,
    // which moves the catalog and every payload.
    let with_folder = |name: &str| {
        let manifest_bytes = word(&good, 16);
        let strings_bytes = word(&good, 32 + 24);
        let added = name.len() + 1;
        let shift = (manifest_bytes + added).div_ceil(4) * 4 - manifest_bytes.div_ceil(4) * 4;
        let mut file = good[..CATALOG].to_vec();
        file.extend_from_slice(name.as_bytes());
        file.resize(CATALOG + shift, 0);
        file.extend_from_slice(&good[CATALOG..]);
        put(&mut file, 16, (manifest_bytes + added) as u32);
        put(&mut file, 32 + 24, (strings_bytes + added) as u32);
        put(&mut file, 32 + 40, 36 | (strings_bytes as u32) << 16);
        put(&mut file, 32 + 44, 1);
        for index in 0..word(&good, ENTRY_COUNT) {
            let at = CATALOG + shift + index * 20 + 8;
            let offset = word(&file, at);
            put(&mut file, at, (offset + shift) as u32);
        }
        file
    };
    assert!(extension::check_package("eq", &with_folder("Presets"), &core()).is_ok());
    assert!(extension::check_package("eq", &with_folder("Abcdefghijklmnop"), &core()).is_ok());
    let cases: Vec<(&str, &str, Vec<u8>)> = vec![
        ("eq", "reserved content folder", with_folder("Extensions")),
        ("eq", "upper-case content folder", with_folder("THEMES")),
        (
            "eq",
            "content folder too long",
            with_folder("Abcdefghijklmnopq"),
        ),
        ("eq", "lower-case content folder", with_content(&good, 0)),
        (
            "eq",
            "content folder with a capital inside",
            with_content(&good, 3),
        ),
        ("eq", "not an rvx", b"ELF\x7f".to_vec()),
        ("eq", "format 0", edit(4, 0)),
        (
            "eq",
            "other firmware",
            edit(12, core().firmware_build_id ^ 1),
        ),
        ("eq", "other api", edit(8, 2 << 16)),
        ("ex", "manifest id", good.clone()),
        ("eq", "fewer entries than modules", edit(ENTRY_COUNT, 1)),
        ("eq", "residents past the catalog", edit(RESIDENT_COUNT, 3)),
        ("eq", "catalog past the end", edit(ENTRY_COUNT, 4000)),
        ("eq", "truncated payload", good[..good.len() - 4].to_vec()),
        ("eq", "unaligned payload", edit(entry(2, 8), 0x105)),
        (
            "eq",
            "payload before the catalog ends",
            edit(entry(2, 8), 28),
        ),
        ("eq", "raw sizes differ", edit(entry(3, 16), 7)),
        ("eq", "module kind after the modules", {
            let mut file = good.clone();
            file[entry(2, 4)] = 0;
            file
        }),
        ("eq", "asset kind among the modules", {
            let mut file = good.clone();
            file[entry(1, 4)] = 1;
            file
        }),
        (
            "eq",
            "two entries share a name",
            edit(entry(3, 0), 0x5c0d8278),
        ),
        ("eq", "module size differs", edit(MAIN_PAYLOAD + 12, 2)),
        ("eq", "entry outside text", edit(MAIN_PAYLOAD + 16, 8)),
        ("eq", "manifest text at the end of the string table", {
            let mut file = good.clone();
            let strings_bytes = word(&good, 32 + 24) as u16;
            file[32 + 28..32 + 30].copy_from_slice(&strings_bytes.to_le_bytes());
            file
        }),
    ];
    for (id, label, file) in cases {
        let (manifest, assets) = with_extension(&f, id, &file, 1);
        assert!(write_signed(&f, &manifest, &assets).is_err(), "{label}");
        assert!(f.load().is_err(), "{label}");
    }
}

/// A theme compiled by the themes repository's `rvt compile`: id `fixture`, a light pack
/// with rendering flags and tint bytes, a dark pack with derive records.
const REFERENCE_THEME: &str = "52565430020080004f2e84ff031616d7030000008000000068030000e8030000900300000f27e1d06669787475726500000000000000000000000000000000000000000000000000466978747572650000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000058030000000000008e5ac8bd00000000ffffffff202020ff0071e3ff3185d0ffffffffff236cc5ff7e8183ff0071e3ff03000000236cc5ff3185d0ff236cc5ffffffffff3185d0ff8944abff8944abff8944abffffffffff8944abffc41e56ffc41e56ffc41e56ffffffffffc41e56ffd02028ffd02028ffd02028ffffffffffd02028ffb84800ffb84800ffb84800ffffffffffb84800ff1e803aff1e803aff1e803affffffffff1e803aff007378ff007378ff007378ffffffffff007378ff826500ff826500ff826500ffffffffff826500ff5c6066ff5c6066ff5c6066ffffffffff5c6066ff000000000000000000000000000000000000000000000000000000000000000082000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008080808080808080808080808080808080808080808000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008003000003000000c4fb1e1000000000101010ffe8e8e8ff00a1ffff00a1ffffffffffff00a1ffff383838ff00a1ffff0000000000a1ffff00a1ffff00a1ffffffffffff00a1ffff964ebcffda8fffff964ebcffffffffffda8fffffce2660ffff6491ffce2660ffffffffffff6491ffd42830ffff6961ffd42830ffffffffffff6961ffbe4c00ffff9f0affbe4c00ffffffffffff9f0aff20863eff30d158ff20863effffffffff30d158ff00787dff40d2d7ff00787dffffffffff40d2d7ff846700ffffd60aff846700ffffffffffffd60aff64686effbec2c8ff64686effffffffffbec2c8ff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000f70000000300000000000000f9000000030000000000000014010000020000000400000011010000";

#[test]
fn theme_layout_matches_the_rvt_tool() {
    let file = unhex(REFERENCE_THEME);
    assert!(theme::check_theme("fixture", &file, &core()).is_ok());
    assert!(theme::check_theme("flat", &file, &core()).is_err());
    let other = Core {
        catalog_fingerprint: "0000000000000000".into(),
        ..core()
    };
    assert!(theme::check_theme("fixture", &file, &other).is_err());
    // A pack without records is a pack header and the style prefix.
    let light = word(&file, LIGHT);
    assert_eq!(word(&file, LIGHT + 4), theme::MIN_PACK);
    assert_eq!(word(&file, light), theme::MIN_PACK - 16);
    assert_eq!(word(&file, light + 4), 0);
    assert!(word(&file, DARK + 4) > theme::MIN_PACK);
}

#[test]
fn themes_are_validated_and_listed_after_the_extensions() {
    let f = Fixture::new();
    let good = theme("flat", b"Flat", 3);
    assert!(theme::check_theme("flat", &good, &core()).is_ok());
    assert!(theme::check_theme("dusk", &theme("dusk", "D\u{fc}sk".as_bytes(), 2), &core()).is_ok());
    let (mut manifest, mut assets) = with_extension(&f, "eq", &reference(), 1);
    let (themed, theme_assets) = with_theme(&f, "flat", &good);
    manifest.themes = themed.themes;
    manifest.assets.extend(themed.assets);
    assets.extend(theme_assets);
    write_signed(&f, &manifest, &assets).unwrap();
    let bundle = f.load().unwrap();
    let files: Vec<_> = bundle
        .extension_files()
        .into_iter()
        .chain(bundle.theme_files())
        .map(|file| (file.id.to_owned(), file.destination))
        .collect();
    assert_eq!(
        files,
        [
            ("eq".to_owned(), "/Revola/Extensions/eq.rvx".to_owned()),
            ("flat".to_owned(), "/Revola/Themes/flat.rvt".to_owned()),
        ]
    );
    assert_eq!(bundle.theme_bytes(), good.len());
    let (manifest, assets) = with_theme(&f, "flat", &good);
    write_signed(&f, &manifest, &assets).unwrap();
    assert!(f.load().is_ok());

    // Manifest-level rejections: core profile, id, format, version, asset.
    let (mut manifest, _) = with_theme(&f, "flat", &good);
    manifest.core = None;
    assert!(manifest.validate("0.2.0").is_err());
    let (mut manifest, _) = with_theme(&f, "flat", &good);
    manifest.core.as_mut().unwrap().catalog_fingerprint = "4f2e84ff031616d".into();
    assert!(manifest.validate("0.2.0").is_err());
    for id in ["Flat", "classic", "-flat", ""] {
        let (manifest, _) = with_theme(&f, id, &good);
        assert!(manifest.validate("0.2.0").is_err(), "{id:?}");
    }
    for (format, version) in [("rvt", "1.0"), (THEME_FORMAT, ""), (THEME_FORMAT, "1\n0")] {
        let (mut manifest, _) = with_theme(&f, "flat", &good);
        let theme = manifest.themes.get_mut("flat").unwrap();
        theme.format = format.into();
        theme.version = version.into();
        assert!(manifest.validate("0.2.0").is_err(), "{format} {version:?}");
    }
    let (mut manifest, _) = with_theme(&f, "flat", &good);
    manifest.themes.get_mut("flat").unwrap().file = "0".repeat(64);
    assert!(manifest.validate("0.2.0").is_err());

    // Content-level rejections happen before signing and at load: the reader's checks.
    let edit = |at: usize, value: u32| {
        let mut file = good.clone();
        put(&mut file, at, value);
        file
    };
    let sealed = |at: usize, value: u32| {
        let mut file = edit(at, value);
        put(&mut file, HEADER_CRC, 0);
        let crc = crc32fast::hash(&file[..128]);
        put(&mut file, HEADER_CRC, crc);
        file
    };
    assert!(theme::check_theme("flat", &sealed(4, 2 | 128 << 16), &core()).is_ok());
    let cases: Vec<(&str, &str, Vec<u8>)> = vec![
        ("flat", "not an rvt", b"RVX0".repeat(40)),
        ("flat", "short file", good[..100].to_vec()),
        ("flat", "version 0", sealed(4, 128 << 16)),
        ("flat", "version 1", sealed(4, 1 | 128 << 16)),
        ("flat", "version 3", sealed(4, 3 | 128 << 16)),
        ("flat", "header smaller than 128", sealed(4, 2 | 64 << 16)),
        ("flat", "header past the file", sealed(4, 2 | 1024 << 16)),
        ("flat", "unaligned header", sealed(4, 2 | 130 << 16)),
        ("flat", "tampered checksum", edit(HEADER_CRC, 0)),
        ("flat", "other catalog", sealed(8, 0)),
        (
            "flat",
            "name changed without resealing",
            edit(72, 0x74616c66),
        ),
        ("flat", "no appearance", sealed(APPEARANCES, 0)),
        ("flat", "unknown appearance", sealed(APPEARANCES, 4)),
        ("flat", "light placed but absent", sealed(APPEARANCES, 2)),
        ("flat", "light before the header ends", sealed(LIGHT, 64)),
        ("flat", "unaligned light", sealed(LIGHT, 130)),
        (
            "flat",
            "light smaller than a pack header",
            sealed(LIGHT + 4, 8),
        ),
        (
            "flat",
            "dark past the end",
            sealed(DARK + 4, PACK as u32 + 4),
        ),
        (
            "flat",
            "dark at the end of memory",
            sealed(DARK, u32::MAX - 8),
        ),
        ("dark", "id differs from the key", good.clone()),
        (
            "flat",
            "id without a terminator",
            theme(&"a".repeat(32), b"Flat", 3),
        ),
        ("flat", "empty name", theme("flat", b"", 3)),
        (
            "flat",
            "control character in the name",
            theme("flat", b"Fl\x01t", 3),
        ),
        (
            "flat",
            "name without a terminator",
            theme("flat", &[b'x'; 48], 3),
        ),
        ("flat", "invalid utf-8 name", theme("flat", b"\xff", 3)),
    ];
    for (id, label, file) in cases {
        assert!(theme::check_theme(id, &file, &core()).is_err(), "{label}");
        let (manifest, assets) = with_theme(&f, id, &file);
        assert!(write_signed(&f, &manifest, &assets).is_err(), "{label}");
        assert!(f.load().is_err(), "{label}");
    }
    let truncated = &good[..good.len() - 1];
    assert!(theme::check_theme("flat", truncated, &core()).is_err());
}

#[test]
fn python_export_loads_as_directory_and_zip() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../usb-helper/tests/fixtures");
    let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/bundle.py");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("bundle");
    let result = std::process::Command::new("python3")
        .arg(&tool)
        .arg("--minimum-installer-version")
        .arg("0.2.0")
        .arg("--helper")
        .arg(&fixture)
        .args(["--version", "0.2.0-dev.1+local.01", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let public = sign_directory(&out, &"18".repeat(32), "0.2.0").unwrap();
    let bundle =
        VerifiedBundle::load(&out, &TrustedKey::from_hex(&public).unwrap(), "0.2.0").unwrap();
    assert_eq!(
        bundle.file("usb_helper", "image").unwrap(),
        fs::read(fixture.join("upload.dfu")).unwrap()
    );
    let archive = tmp.path().join("bundle.zip");
    let result = std::process::Command::new("python3")
        .arg(&tool)
        .arg("--pack")
        .arg(&out)
        .arg("--out")
        .arg(&archive)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let packed =
        VerifiedBundle::load_zip(&archive, &TrustedKey::from_hex(&public).unwrap(), "0.2.0")
            .unwrap();
    assert_eq!(packed.digest(), bundle.digest());
    assert_eq!(
        packed.file("usb_helper", "image").unwrap(),
        bundle.file("usb_helper", "image").unwrap()
    );
}

#[cfg(unix)]
#[test]
fn assets_cannot_be_symlinks() {
    let f = Fixture::new();
    let bundle = f.load().unwrap();
    let hash = bundle.manifest().assets.keys().next().unwrap();
    let path = f.dir.path().join(asset_filename(hash));
    let moved = f.dir.path().join("original");
    fs::rename(&path, &moved).unwrap();
    std::os::unix::fs::symlink(moved, path).unwrap();
    assert!(f.load().is_err());
}
