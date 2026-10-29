use super::super::{capture_root, confirm_absent};
use super::TestDirectory;
use crate::settings::SavedSelection;
use drip_core::SourcePath;

#[test]
fn missing_state_subtrees_never_authorize_server_deletion() {
    let directory = TestDirectory::new();
    let app = directory.path.join("app");
    std::fs::create_dir(&app).unwrap();
    for selected in [&app, &directory.path] {
        let root = capture_root(selected).unwrap();
        let saved = SavedSelection {
            id: "state-exclusion".into(),
            source: root.source,
            root_kind: root.kind,
            volume_identity: root.volume_identity,
            root_identity: None,
            active: true,
        };
        for name in [
            r"drip.state.sqlite3",
            r"drip.state.backup.missing\private.bin",
            r"DRIP.STATE.corrupt.missing\nested\private.bin",
        ] {
            let target = SourcePath::try_from_path(&app.join(name)).unwrap();
            assert!(!confirm_absent(&saved, &target, &app).unwrap(), "{name}");
        }
        let ordinary = SourcePath::try_from_path(&app.join("drip.stateful.user.txt")).unwrap();
        assert!(confirm_absent(&saved, &ordinary, &app).unwrap());
    }
}
