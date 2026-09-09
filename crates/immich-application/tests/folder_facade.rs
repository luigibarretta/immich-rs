use std::path::PathBuf;

use immich_rs_application::{
    APPLICATION_PROGRESS_SCHEMA_VERSION, ApplicationProgressEvent, ApplicationProgressObserver,
    FolderPlanRequest, FolderScanConfig, plan_folder,
};
use immich_rs_core::{NeverCancel, PROGRESS_EVENT_SCHEMA_VERSION};
use immich_rs_sources::{NoProgress, scan_folder};

#[derive(Default)]
struct RecordingProgress(Vec<ApplicationProgressEvent>);

impl ApplicationProgressObserver for RecordingProgress {
    fn observe(&mut self, event: ApplicationProgressEvent) {
        self.0.push(event);
    }
}

fn synthetic_source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/example-source")
}

#[test]
fn folder_facade_preserves_plan_bytes_and_nests_scan_progress()
-> Result<(), Box<dyn std::error::Error>> {
    let request = FolderPlanRequest {
        root: synthetic_source(),
        label: "application-facade".to_owned(),
        config: FolderScanConfig::default(),
    };
    let direct = scan_folder(
        &request.root,
        &request.label,
        &request.config,
        &NeverCancel,
        &mut NoProgress,
    )?;
    let mut progress = RecordingProgress::default();
    let facade = plan_folder(&request, &NeverCancel, &mut progress)?;

    assert_eq!(
        serde_json::to_vec_pretty(&direct)?,
        serde_json::to_vec_pretty(&facade)?
    );
    assert!(!progress.0.is_empty());
    assert!(progress.0.iter().all(|item| matches!(
        item,
        ApplicationProgressEvent::Scan { schema_version, event }
            if *schema_version == APPLICATION_PROGRESS_SCHEMA_VERSION
                && event.schema_version == PROGRESS_EVENT_SCHEMA_VERSION
    )));
    Ok(())
}
