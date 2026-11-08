use cyclone_core::{
    capture, inspect, restore, CaptureOptions, EncryptionKey, Profile, RestoreOptions,
};
use tempfile::tempdir;

#[test]
fn capture_inspect_restore_round_trip() {
    let dir = tempdir().unwrap();
    let fixture = dir.path().join("sample.gitconfig");
    std::fs::write(&fixture, "[user]\n\tname = Test\n").unwrap();

    let profile = Profile {
        schema_version: 1,
        capture_roots: vec![fixture],
        tool_declarations: vec![],
        secret_references: vec![],
        notes: None,
    };

    let image = dir.path().join("image");
    let key = EncryptionKey::random();

    let report = capture(CaptureOptions {
        profile: &profile,
        output: &image,
        encryption_key: &key,
        dry_run: false,
    })
    .unwrap();
    assert_eq!(report.component_count, 1);

    let inspected = inspect(&image).unwrap();
    assert_eq!(inspected.component_count, 1);

    let dry = restore(RestoreOptions {
        image: &image,
        encryption_key: Some(&key),
        target_root: None,
        dry_run: true,
        overwrite: false,
    })
    .unwrap();
    assert!(!dry.planned_actions.is_empty());

    let restore_parent = dir.path().join("out");
    std::fs::create_dir_all(&restore_parent).unwrap();
    let applied = restore(RestoreOptions {
        image: &image,
        encryption_key: Some(&key),
        target_root: Some(restore_parent.clone()),
        dry_run: false,
        overwrite: true,
    })
    .unwrap();
    assert_eq!(applied.applied_files, 1);
    let restored = restore_parent.join("sample.gitconfig");
    let body = std::fs::read_to_string(restored).unwrap();
    assert!(body.contains("name = Test"));
}
