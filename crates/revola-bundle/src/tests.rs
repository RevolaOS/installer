// SPDX-License-Identifier: GPL-3.0-only

use super::*;

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
const REFERENCE_PACKAGE: &str = "525658300100000007000100ffb60e666b0000000400000001000000b77ae67652564d460200000007000100010000000200000000000000370000000000030006000c001200170024000000000029000101300065710045510042616e647300302e312e30005061756c0047504c2d332e302d6f6e6c79006d61696e00656469746f7200656469746f72000008e290ea00000000dc000000240000002400000078820d5c00000000080100002000000020000000f01540e602010000000100000500000028000000df9b9c4a010000002801000006000000060000000800000004000000040000000100000001000000000102030405060708090a0b080000000211f0001500000008000000040000000000000000000000010000000000000000000000000000000001020304050000";
/// Offsets in REFERENCE_PACKAGE: the header's entry and resident counts, the catalog, and
/// the first module payload.
const ENTRY_COUNT: usize = 20;
const RESIDENT_COUNT: usize = 24;
const CATALOG: usize = 32 + 108;
const MAIN_PAYLOAD: usize = 0xdc;

fn unhex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn core() -> Core {
    Core {
        api: 1,
        firmware_build_id: extension::firmware_build_id(
            &revola_device::firmware::current().inputs["osos.bin"].sha256,
        )
        .unwrap(),
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
        .cloned()
        .collect();
    manifest.assets.retain(|hash, _| referenced.contains(hash));
    (manifest, assets)
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
    let cases: Vec<(&str, &str, Vec<u8>)> = vec![
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
    ];
    for (id, label, file) in cases {
        let (manifest, assets) = with_extension(&f, id, &file, 1);
        assert!(write_signed(&f, &manifest, &assets).is_err(), "{label}");
        assert!(f.load().is_err(), "{label}");
    }
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
