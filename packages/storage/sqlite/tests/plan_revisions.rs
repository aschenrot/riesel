//! Production V2 trusted-local adapter: actual validated plans, exact
//! editor binding, independent SQLite connections and OS-process collisions.
//! This is NOT a hostile-client authorization, runtime activation, or
//! power-loss/exactly-once effects proof.

use riesel_app::{
    AccessMode, CheckedSourceHandoff, EditorOperationDescriptor, ExecutionPlanDraft,
    OperationKey, OperationNode, OperationRegistry, PlanAuthoringSource, PlanLayoutDraft,
    PlanLayoutPoint, PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate,
    S2DocumentAccess, S2DocumentAccessMode, S2DocumentLimits, S2OperationDocument,
    S2PlanDocument, S2SourceDocument, ValidatedExecutionPlan, inspect_s2_source,
};
use riesel_storage_sqlite::SqlitePlanRevisionRepository;
use rusqlite::Connection;
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

const PLAN: &str = "v2.plan";
const STAMP: &str = "trusted-local-app.v1";

type Handler = dyn Fn() + Send + Sync;

fn key() -> OperationKey {
    OperationKey::new("v2.work", 1).unwrap()
}

fn plan(revision: u64, extra: bool) -> ValidatedExecutionPlan {
    let draft = ExecutionPlanDraft::new(PLAN, revision).operation(
        OperationNode::new("work", key()).access("data", AccessMode::Write),
    );
    let draft = if extra {
        draft.operation(OperationNode::new("extra", key()).after("work").access("data",AccessMode::Write))
    } else {
        draft
    };
    draft.validate().unwrap()
}

fn registry() -> OperationRegistry<Handler> {
    let mut handlers = OperationRegistry::<Handler>::new();
    handlers.register_editor_operation(
        EditorOperationDescriptor::new(key()).require_access("data",AccessMode::Write),
        Arc::new(|| {}),
    ).unwrap();
    handlers
}

fn s2(revision: u64) -> S2PlanDocument {
    let mut operation = S2OperationDocument::new("work",&key(),1);
    operation.resources.push(S2DocumentAccess {
        resource:"data".into(),
        mode:S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new(PLAN,revision).operation(operation)
}

fn code_candidate(revision: u64) -> PlanSemanticCandidate {
    PlanSemanticCandidate::from_rust(&plan(revision,false)).unwrap()
}

fn repo(path: &Path) -> SqlitePlanRevisionRepository {
    SqlitePlanRevisionRepository::open(path, STAMP).unwrap()
}

fn layout(id: &str,anchor: u64,with_extra: bool) -> PlanLayoutDraft {
    let mut node_positions=BTreeMap::new();
    node_positions.insert("work".into(),PlanLayoutPoint{x:10,y:20});
    if with_extra {
        node_positions.insert("extra".into(),PlanLayoutPoint{x:30,y:40});
    }
    PlanLayoutDraft{layout_id:id.into(),semantic_anchor:anchor,node_positions}
}

#[test]
fn persisted_immutable_semantic_history_and_independent_layouts() {
    let temp=tempdir().unwrap();
    let path=temp.path().join("plans.sqlite");
    let store=repo(&path);
    let head=store.initialize(&code_candidate(1),PlanAuthoringSource::Rust,STAMP).unwrap();
    assert_eq!(head.semantic_revision,1);
    let first_layout=store.save_layout(&head,None,&layout("desktop",1,false)).unwrap();
    assert_eq!(first_layout.version,1);
    let second_stream=store.save_layout(&head,None,&layout("mobile",1,false)).unwrap();
    assert_eq!(second_stream.version,1);
    let updated=store.append(&head,&PlanSemanticCandidate::from_rust(&plan(2,true)).unwrap()).unwrap();
    assert_eq!(updated.semantic_revision,2);
    assert_eq!(store.semantic(PLAN,1).unwrap().node_ids,vec!["work"]);
    assert_eq!(store.semantic(PLAN,2).unwrap().node_ids,vec!["work","extra"]);
    let rebased=store.save_layout(&updated,Some(1),&layout("desktop",2,true)).unwrap();
    assert_eq!(rebased.version,2);
    assert_eq!(rebased.layout.semantic_anchor,2);
    assert_eq!(store.layout(PLAN,"desktop",1).unwrap().layout.semantic_anchor,1);
    assert_eq!(store.layout(PLAN,"mobile",1).unwrap().version,1);
    assert_eq!(store.head(PLAN).unwrap().semantic_revision,2);
    let reopened=repo(&path);
    assert_eq!(reopened.semantic(PLAN,1).unwrap().node_ids,vec!["work"]);
    assert_eq!(reopened.layout(PLAN,"desktop",2).unwrap().layout.semantic_anchor,2);
    assert_eq!(reopened.head(PLAN).unwrap(),updated);
}

#[test]
fn rejected_layout_rebase_does_not_change_history_or_semantics() {
    let temp=tempdir().unwrap();
    let store=repo(&temp.path().join("plans.sqlite"));
    let head=store.initialize(&code_candidate(1),PlanAuthoringSource::Rust,STAMP).unwrap();
    store.save_layout(&head,None,&layout("desktop",1,false)).unwrap();
    let unknown=layout("desktop",1,true);
    assert!(matches!(
        store.save_layout(&head,Some(1),&unknown),
        Err(PlanRevisionError::Invalid(_))
    ));
    assert!(matches!(
        store.save_layout(&head,Some(0),&layout("desktop",1,false)),
        Err(PlanRevisionError::Conflict)
    ));
    assert!(matches!(
        store.layout(PLAN,"desktop",2),
        Err(PlanRevisionError::NotFound)
    ));
    assert_eq!(store.head(PLAN).unwrap().semantic_revision,1);
}

#[test]
fn checked_source_handoff_fences_semantic_and_layout_writers() {
    let temp=tempdir().unwrap();
    let path=temp.path().join("plans.sqlite");
    let store=repo(&path);
    let original=plan(1,false);
    let head=store.initialize(&code_candidate(1),PlanAuthoringSource::Rust,STAMP).unwrap();
    let handlers=registry();
    let wrong=CheckedSourceHandoff::rust_to_document(
        &original,&s2(2),&handlers,S2DocumentLimits::default(),
    );
    assert!(matches!(wrong,Err(PlanRevisionError::Invalid(_))));
    let unsupported=CheckedSourceHandoff::rust_to_document(
        &plan(1,true),&s2(1),&handlers,S2DocumentLimits::default(),
    );
    assert!(unsupported.is_err());
    let proof=CheckedSourceHandoff::rust_to_document(
        &original,&s2(1),&handlers,S2DocumentLimits::default(),
    ).unwrap();
    let transferred=store.handoff(&head,&proof).unwrap();
    assert_eq!(transferred.source,PlanAuthoringSource::Document);
    assert_eq!(transferred.source_generation,2);
    assert_eq!(transferred.semantic_revision,1);
    assert!(matches!(store.append(&head,&code_candidate(2)),Err(PlanRevisionError::Conflict)));
    assert!(matches!(
        store.save_layout(&head,None,&layout("desktop",1,false)),
        Err(PlanRevisionError::Conflict)
    ));
    let bytes=store.source_document(PLAN,2).unwrap().unwrap();
    assert!(matches!(
        inspect_s2_source(&bytes,S2DocumentLimits::default()).unwrap(),
        S2SourceDocument::Editable(_)
    ));
    let candidate=PlanSemanticCandidate::from_s2(&s2(2),&handlers,S2DocumentLimits::default()).unwrap();
    let new_head=store.append(&transferred,&candidate).unwrap();
    let reverse=CheckedSourceHandoff::document_to_rust(
        &s2(2),&plan(2,false),&handlers,S2DocumentLimits::default()
    ).unwrap();
    let back=store.handoff(&new_head,&reverse).unwrap();
    assert_eq!(back.source,PlanAuthoringSource::Rust);
    assert_eq!(back.source_generation,3);
    assert!(store.source_document(PLAN,3).unwrap().is_none());
    assert_eq!(store.semantic(PLAN,1).unwrap().revision,1);
    assert_eq!(store.head(PLAN).unwrap().semantic_revision,2);
}

#[test]
fn trusted_registration_stamp_mismatch_blocks_mutations_not_local_inspection() {
    let temp=tempdir().unwrap();
    let path=temp.path().join("plans.sqlite");
    let store=repo(&path);
    let head=store.initialize(&code_candidate(1),PlanAuthoringSource::Rust,STAMP).unwrap();
    let incompatible=SqlitePlanRevisionRepository::open(&path,"different-build").unwrap();
    assert_eq!(incompatible.head(PLAN).unwrap(),head);
    assert!(matches!(
        incompatible.append(&head,&code_candidate(2)),
        Err(PlanRevisionError::Conflict)
    ));
    assert!(matches!(
        incompatible.save_layout(&head,None,&layout("desktop",1,false)),
        Err(PlanRevisionError::Conflict)
    ));
    assert_eq!(store.head(PLAN).unwrap().semantic_revision,1);
}

#[test]
fn empty_or_incompatible_database_fails_without_destructive_migration() {
    let dir=tempdir().unwrap();
    let path=dir.path().join("foreign.sqlite");
    let connection=Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=83; CREATE TABLE foreign_data(value TEXT);").unwrap();
    drop(connection);
    assert!(matches!(
        SqlitePlanRevisionRepository::open(&path,STAMP),
        Err(PlanRevisionError::UnsupportedFormat(83))
    ));
    let second=dir.path().join("existing.sqlite");
    let connection=Connection::open(&second).unwrap();
    connection.execute_batch("CREATE TABLE foreign_data(value TEXT);").unwrap();
    drop(connection);
    assert!(matches!(
        SqlitePlanRevisionRepository::open(&second,STAMP),
        Err(PlanRevisionError::UnsupportedFormat(0))
    ));
    assert!(matches!(
        SqlitePlanRevisionRepository::open(":memory:",STAMP),
        Err(PlanRevisionError::Invalid(_))
    ));
}

fn wait_for(path: &Path) {
    let start=Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < Duration::from_secs(18),"child synchronization timeout: {path:?}");
        thread::sleep(Duration::from_millis(12));
    }
}

fn worker(db:&Path, sync:&Path, id:&str, mode:&str) -> Child {
    Command::new(env::current_exe().unwrap())
        .arg("--exact").arg("os_process_worker").arg("--nocapture")
        .env("RIESEL_V2_DB",db)
        .env("RIESEL_V2_SYNC",sync)
        .env("RIESEL_V2_ID",id)
        .env("RIESEL_V2_MODE",mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn().unwrap()
}

#[test]
fn os_process_worker() {
    let Ok(path)=env::var("RIESEL_V2_DB") else {return};
    let sync=PathBuf::from(env::var("RIESEL_V2_SYNC").unwrap());
    let id=env::var("RIESEL_V2_ID").unwrap();
    let mode=env::var("RIESEL_V2_MODE").unwrap();
    let store=repo(Path::new(&path));
    let head=store.head(PLAN).unwrap();
    File::create(sync.join(format!("ready-{id}"))).unwrap();
    wait_for(&sync.join("go"));
    let result=if mode=="semantic" {
        store.append(&head,&code_candidate(2)).map(|_|())
    } else {
        store.save_layout(&head,None,&layout("desktop",1,false)).map(|_|())
    };
    match result {
        Ok(())=>println!("WRITER_OUTCOME=won"),
        Err(PlanRevisionError::Conflict)=>println!("WRITER_OUTCOME=conflict"),
        Err(error)=>panic!("unexpected production adapter result: {error:?}"),
    }
}

fn race(mode:&str) {
    let dir=tempdir().unwrap();
    let db=dir.path().join("plans.sqlite");
    let sync=dir.path().join("sync");
    fs::create_dir(&sync).unwrap();
    let store=repo(&db);
    store.initialize(&code_candidate(1),PlanAuthoringSource::Rust,STAMP).unwrap();
    let workers=vec![
        worker(&db,&sync,"first",mode),
        worker(&db,&sync,"second",mode),
    ];
    wait_for(&sync.join("ready-first"));
    wait_for(&sync.join("ready-second"));
    File::create(sync.join("go")).unwrap();
    let mut accepted=0;
    for child in workers {
        let output=child.wait_with_output().unwrap();
        assert!(output.status.success(),"OS child failed: {}",String::from_utf8_lossy(&output.stderr));
        let stdout=String::from_utf8_lossy(&output.stdout);
        if stdout.contains("WRITER_OUTCOME=won") {
            accepted+=1;
        } else {
            assert!(stdout.contains("WRITER_OUTCOME=conflict"),"{stdout}");
        }
    }
    assert_eq!(accepted,1,"only one real OS process may accept the same predecessor");
    let current=repo(&db);
    if mode=="semantic" {
        assert_eq!(current.head(PLAN).unwrap().semantic_revision,2);
        assert!(matches!(current.layout(PLAN,"desktop",1),Err(PlanRevisionError::NotFound)));
    } else {
        assert_eq!(current.head(PLAN).unwrap().semantic_revision,1);
        assert_eq!(current.layout(PLAN,"desktop",1).unwrap().version,1);
    }
}

#[test]
fn two_independent_os_processes_race_on_production_semantic_cas() {
    race("semantic");
}

#[test]
fn two_independent_os_processes_race_on_production_layout_cas() {
    race("layout");
}
