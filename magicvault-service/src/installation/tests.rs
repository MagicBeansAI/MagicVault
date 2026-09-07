use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};

struct Fixture {
    _temp: tempfile::TempDir,
    app: PathBuf,
    vault: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let source = base.join("bundle");
        fs::create_dir(&source).unwrap();
        for name in ["bin", "extension", "examples"] {
            fs::create_dir(source.join(name)).unwrap();
        }
        let mut files = BTreeMap::new();
        for name in FILES {
            let bytes = format!("synthetic artifact {name}");
            fs::write(source.join(name), &bytes).unwrap();
            files.insert(
                (*name).into(),
                Artifact {
                    sha256: hex::encode(Sha256::digest(bytes.as_bytes())),
                    bytes: bytes.len() as u64,
                    executable: name.starts_with("bin/"),
                },
            );
        }
        fs::write(
            source.join("bundle.json"),
            serde_json::to_vec(&Bundle {
                format_version: 1,
                version: "0.5.0".into(),
                platform: "darwin-arm64".into(),
                files,
            })
            .unwrap(),
        )
        .unwrap();
        Self {
            _temp: temp,
            app: base.join("app"),
            vault: base.join("vault"),
            source,
        }
    }
    fn open(&self) -> Installation {
        Installation::open(&self.app, &self.vault, true).unwrap()
    }
}

#[test]
fn bundle_copy_is_private_durable_and_independent_of_source() {
    let f = Fixture::new();
    let app = f.open();
    assert!(app.current().unwrap().is_none());
    app.activate(app.stage(&f.source, "0.5.0").unwrap())
        .unwrap();
    assert_eq!(app.current().unwrap().unwrap().version, "0.5.0");
    assert_eq!(
        fs::metadata(app.executable("magicvault").unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    fs::write(f.source.join("bin/magicvault"), "changed npm cache").unwrap();
    assert!(app.current().is_ok());
    assert!(!f.vault.exists()); // Installer has no vault/keychain side effects.
}

#[test]
fn concurrent_or_wrong_instance_installers_cannot_mutate() {
    let f = Fixture::new();
    let _held = f.open();
    assert!(matches!(
        Installation::open(&f.app, &f.vault, true),
        Err(ErrorCode::Busy)
    ));
    assert!(Installation::open(&f.app, &f.vault.with_file_name("other"), true).is_err());
}

#[test]
fn unknown_directory_and_read_only_missing_installation_are_untouched() {
    let f = Fixture::new();
    assert!(Installation::open(&f.app, &f.vault, false).is_err());
    assert!(!f.app.exists());
    fs::create_dir(&f.app).unwrap();
    fs::set_permissions(&f.app, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(f.app.join("user-data"), "preserve").unwrap();
    assert!(Installation::open(&f.app, &f.vault, true).is_err());
    assert_eq!(fs::read_dir(&f.app).unwrap().count(), 1);
}

#[test]
fn roots_cannot_overlap_or_follow_leaf_symlinks() {
    let f = Fixture::new();
    assert!(Installation::open(&f.app, &f.app, true).is_err());
    symlink(&f.source, &f.app).unwrap();
    assert!(Installation::open(&f.app, &f.vault, true).is_err());
}

#[test]
fn corruption_and_partial_bundles_never_replace_current() {
    let f = Fixture::new();
    let app = f.open();
    app.activate(app.stage(&f.source, "0.5.0").unwrap())
        .unwrap();
    let before = fs::read_link(f.app.join("current")).unwrap();
    fs::write(f.source.join("bin/magicvault"), "tamper").unwrap();
    assert!(app.stage(&f.source, "0.5.0").is_err());
    assert_eq!(fs::read_link(f.app.join("current")).unwrap(), before);
    assert!(app.current().is_ok());
}

#[test]
fn malicious_manifest_paths_versions_and_symlink_assets_are_refused() {
    let f = Fixture::new();
    let app = f.open();
    assert!(app.stage(&f.source, "0.6.0").is_err());
    let mut bundle = manifest(&f.source).unwrap();
    bundle
        .files
        .insert("../outside".into(), bundle.files["bin/magicvault"].clone());
    fs::write(
        f.source.join("bundle.json"),
        serde_json::to_vec(&bundle).unwrap(),
    )
    .unwrap();
    assert!(app.stage(&f.source, "0.5.0").is_err());
    bundle.files.remove("../outside");
    fs::write(
        f.source.join("bundle.json"),
        serde_json::to_vec(&bundle).unwrap(),
    )
    .unwrap();
    fs::remove_file(f.source.join("bin/magicvault")).unwrap();
    symlink(
        f.source.join("bin/magicvault-mcp"),
        f.source.join("bin/magicvault"),
    )
    .unwrap();
    assert!(app.stage(&f.source, "0.5.0").is_err());
}

#[test]
fn modified_active_bundle_and_escaping_pointer_fail_closed() {
    let f = Fixture::new();
    let app = f.open();
    app.activate(app.stage(&f.source, "0.5.0").unwrap())
        .unwrap();
    fs::write(app.executable("magicvault").unwrap(), "tamper").unwrap();
    assert!(app.current().is_err());
    fs::remove_file(f.app.join("current")).unwrap();
    symlink("../bundle", f.app.join("current")).unwrap();
    assert!(app.current().is_err());
}

#[test]
fn retirement_is_recoverable_and_preserves_vault_data() {
    let f = Fixture::new();
    let app = f.open();
    app.activate(app.stage(&f.source, "0.5.0").unwrap())
        .unwrap();
    fs::create_dir(&f.vault).unwrap();
    fs::write(f.vault.join("sentinel"), "keep").unwrap();
    let archive = app.retire().unwrap();
    assert!(!f.app.exists());
    assert!(archive.join("current/bin/magicvault").exists());
    assert_eq!(
        fs::read_to_string(f.vault.join("sentinel")).unwrap(),
        "keep"
    );
}

#[test]
fn stage_is_revalidated_at_activation_and_old_versions_are_retained() {
    let f = Fixture::new();
    let app = f.open();
    let first = app.stage(&f.source, "0.5.0").unwrap();
    let old = first.relative.clone();
    app.activate(first).unwrap();
    let second = app.stage(&f.source, "0.5.0").unwrap();
    fs::write(
        f.app.join(&second.relative).join("bin/magicvault"),
        "changed after stage",
    )
    .unwrap();
    assert!(app.activate(second).is_err());
    assert_eq!(fs::read_link(f.app.join("current")).unwrap(), old);
    app.activate(app.stage(&f.source, "0.5.0").unwrap())
        .unwrap();
    assert!(f.app.join(old).exists());
}

#[test]
fn fifo_source_is_refused_without_waiting_for_a_writer() {
    let f = Fixture::new();
    let app = f.open();
    let file = f.source.join("bin/magicvault");
    fs::remove_file(&file).unwrap();
    let cpath = std::ffi::CString::new(file.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    assert!(app.stage(&f.source, "0.5.0").is_err());
}
