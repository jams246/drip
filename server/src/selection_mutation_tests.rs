use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use drip_core::{
    SensitiveString, SourcePath,
    protocol::{EnrollmentRequest, EntryKind, Selection, SelectionUpdate},
};

use crate::{DeviceIdentity, Store, StoreError, StoreResult};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    store: Store,
    credential: String,
    selection_id: String,
}

impl Fixture {
    fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".verification/phase-02/server-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("{}-mutation-{suffix}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("state"), root.join("mirror")).unwrap();
        let token = store.create_enrollment_token().unwrap();
        let device = store
            .enroll(EnrollmentRequest {
                token: SensitiveString::new(token),
                label: "Mutation test".to_owned(),
            })
            .unwrap();
        let selection_id = "a".repeat(32);
        store
            .update_selections(
                device.device_credential.expose_secret(),
                SelectionUpdate {
                    expected_generation: 0,
                    selections: vec![Selection {
                        id: selection_id.clone(),
                        source: SourcePath::new('C', vec!["file.txt".to_owned()]).unwrap(),
                        root_kind: EntryKind::File,
                        active: true,
                    }],
                },
            )
            .unwrap();
        Self {
            root,
            store,
            credential: device.device_credential.expose_secret().to_owned(),
            selection_id,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn mutation_helper_rechecks_selection_generation_and_activity_in_one_transaction() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .store
            .with_authorized_selection_mutation(
                &fixture.credential,
                &fixture.selection_id,
                1,
                no_op,
            )
            .is_ok()
    );
    assert!(matches!(
        fixture.store.with_authorized_selection_mutation(
            &fixture.credential,
            &fixture.selection_id,
            0,
            no_op,
        ),
        Err(StoreError::Conflict)
    ));
    fixture
        .store
        .update_selections(
            &fixture.credential,
            SelectionUpdate {
                expected_generation: 1,
                selections: Vec::new(),
            },
        )
        .unwrap();
    assert!(matches!(
        fixture.store.with_authorized_selection_mutation(
            &fixture.credential,
            &fixture.selection_id,
            2,
            no_op,
        ),
        Err(StoreError::DisallowedNamespace)
    ));
}

fn no_op(
    _: &rusqlite::Transaction<'_>,
    _: &DeviceIdentity,
    _: &Selection,
    _: &cap_std::fs::Dir,
) -> StoreResult<()> {
    Ok(())
}
