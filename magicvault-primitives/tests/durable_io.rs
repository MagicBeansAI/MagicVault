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

#[cfg(unix)]
#[test]
fn bare_relative_filename_syncs_the_current_directory_not_an_empty_path() {
    // No chdir or writes to process-global CWD; exercise only parent selection.
    magicvault_primitives::durable_io::sync_parent_dir_blocking(std::path::Path::new("relative-record")).unwrap();
}

#[cfg(unix)]
#[test]
fn requested_final_mode_is_preserved_exactly_after_replacement() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("record");
    write_bytes_durably_with_mode_sync(&path,b"first",Some(0o600)).unwrap();
    write_bytes_durably_with_mode_sync(&path,b"second",Some(0o640)).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,0o640);
    assert_eq!(std::fs::read(path).unwrap(),b"second");
}
