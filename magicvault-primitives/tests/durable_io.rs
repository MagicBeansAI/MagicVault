use magicvault_primitives::durable_io::write_bytes_durably_with_mode_sync;

#[test]
fn atomic_replacement_keeps_complete_bytes_and_leaves_no_staging_files() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("record");
    write_bytes_durably_with_mode_sync(&target, b"old", Some(0o600)).unwrap();
    write_bytes_durably_with_mode_sync(&target, b"new-complete-record", Some(0o600)).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"new-complete-record");
    let names = std::fs::read_dir(root.path()).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from("record")]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(target).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn failed_publication_does_not_replace_the_destination_or_leak_a_temp() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("directory");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("retained"), b"previous-state").unwrap();
    assert!(write_bytes_durably_with_mode_sync(&target, b"rejected", Some(0o600)).is_err());
    assert_eq!(std::fs::read(target.join("retained")).unwrap(), b"previous-state");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
