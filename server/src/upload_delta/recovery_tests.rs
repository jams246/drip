use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use drip_core::protocol::{EnrollmentRequest, EntryKind, Selection, SelectionUpdate};
use drip_core::{SensitiveString, SourcePath};

use crate::{Store, upload_test_support::CheckpointHookGuard};

mod support;

use support::{
    commit_delta, descriptors, digest, manifest_count, metadata, mirror_path, plan, prepare,
    pseudo_random, root, send_missing,
};

#[tokio::test]
async fn restart_reindexes_actual_file_after_rename_before_metadata_commit() {
    let (root, _cleanup) = root();
    let state_dir = root.join("state");
    let mirror_dir = root.join("mirror");
    let store = Store::open(&state_dir, &mirror_dir).unwrap();
    let token = store.create_enrollment_token().unwrap();
    let device = store
        .enroll(EnrollmentRequest {
            token: SensitiveString::new(token),
            label: "Delta restart".to_owned(),
        })
        .unwrap();
    let credential = device.device_credential.expose_secret().to_owned();
    let selection_id = "a".repeat(32);
    let source = SourcePath::new('C', vec!["restart.bin".to_owned()]).unwrap();
    let generation = store
        .update_selections(
            &credential,
            SelectionUpdate {
                expected_generation: 0,
                selections: vec![Selection {
                    id: selection_id.clone(),
                    source: source.clone(),
                    root_kind: EntryKind::File,
                    active: true,
                }],
            },
        )
        .unwrap()
        .generation;

    let baseline = pseudo_random(2 * 1024 * 1024);
    commit_delta(
        &store,
        &credential,
        &selection_id,
        generation,
        &source,
        &baseline,
    )
    .await;
    let old = metadata(&store, &device.pc_id, &source);
    let baseline_revision = old.0;
    assert_eq!(old.1, digest(&baseline).to_hex());
    let cached_manifest_count = manifest_count(&store, &device.pc_id, &source, baseline_revision);
    assert_eq!(cached_manifest_count, 1);

    let mut replacement = baseline.clone();
    let midpoint = replacement.len() / 2;
    replacement[midpoint] ^= 1;
    let pending = prepare(
        &store,
        &credential,
        &selection_id,
        generation,
        &source,
        &replacement,
    );
    let descriptors = descriptors(&replacement);
    let planned = store
        .plan_upload(&credential, &pending.upload_id, plan(descriptors.clone()))
        .unwrap();
    assert!(!planned.missing_ordinals.is_empty());
    send_missing(
        &store,
        &credential,
        &pending.upload_id,
        &replacement,
        &descriptors,
        &planned.missing_ordinals,
    )
    .await;

    let interrupted_store = store.clone();
    let interrupted_credential = credential.clone();
    let upload_id = pending.upload_id.clone();
    let target_thread = Arc::new(Mutex::new(None::<thread::ThreadId>));
    let hook_thread = Arc::clone(&target_thread);
    let hits = Arc::new(AtomicUsize::new(0));
    let hook_hits = Arc::clone(&hits);
    let hook = CheckpointHookGuard::new(Arc::new(move |checkpoint| {
        let is_target = *hook_thread.lock().unwrap() == Some(thread::current().id());
        if checkpoint == "after_rename" && is_target {
            hook_hits.fetch_add(1, Ordering::SeqCst);
            panic!("simulated interruption after file rename");
        }
    }));
    let worker_target = Arc::clone(&target_thread);
    let interrupted = thread::spawn(move || {
        *worker_target.lock().unwrap() = Some(thread::current().id());
        interrupted_store.commit_upload(&interrupted_credential, &upload_id)
    })
    .join();
    drop(hook);
    assert!(interrupted.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(mirror_path(&root, &device.mirror_directory, &source)).unwrap(),
        replacement
    );
    assert_eq!(metadata(&store, &device.pc_id, &source), old);

    store.prepare_daemon().unwrap();
    assert!(matches!(
        store.get_upload_state(&credential, &pending.upload_id),
        Err(crate::StoreError::NotFound)
    ));
    let recovered = Store::open(&state_dir, &mirror_dir).unwrap();
    recovered.prepare_daemon().unwrap();
    let prepared = prepare(
        &recovered,
        &credential,
        &selection_id,
        generation,
        &source,
        &replacement,
    );
    assert!(prepared.base_revision.is_some());
    assert_eq!(
        manifest_count(&recovered, &device.pc_id, &source, baseline_revision),
        0
    );
    let recovery_plan = recovered
        .plan_upload(&credential, &prepared.upload_id, plan(descriptors))
        .unwrap();
    assert!(recovery_plan.missing_ordinals.is_empty());
    recovered
        .commit_upload(&credential, &prepared.upload_id)
        .unwrap();
    assert_eq!(
        std::fs::read(mirror_path(&root, &device.mirror_directory, &source)).unwrap(),
        replacement
    );
}
