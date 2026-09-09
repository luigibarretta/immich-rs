use std::path::PathBuf;

use immich_rs_application::{
    ApplicationError, ApplicationProgressObserver, Cancellation, FolderPlanRequest,
    FolderUploadPlanRequest, ImmichReadClient, NormalizedPlan, ScanError, SourcePlanRequest,
    SourceUploadPlanRequest, UploadApplyRequest, UploadDryRunRequest, UploadExecutionConfig,
    UploadPlan, plan_apple_photos, plan_apple_photos_upload, plan_folder, plan_folder_upload,
    plan_google_takeout, plan_google_takeout_upload, plan_picasa, plan_picasa_upload,
};

use crate::profiles::{ResolvedSourceProfile, SourceKind, SourceSettings};
use crate::state_store::HistoryKind;

pub(super) fn scan(
    source: &ResolvedSourceProfile,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    let root = source
        .inputs()
        .first()
        .cloned()
        .ok_or(ScanError::InvalidRoot)?;
    match source.settings() {
        SourceSettings::Folder(config) => plan_folder(
            &FolderPlanRequest {
                root,
                label: source.label().to_owned(),
                config: config.clone(),
            },
            cancellation,
            observer,
        ),
        SourceSettings::GoogleTakeout(config) => plan_google_takeout(
            &SourcePlanRequest {
                inputs: source.inputs().to_vec(),
                label: source.label().to_owned(),
                config: config.source.clone(),
            },
            cancellation,
            observer,
        ),
        SourceSettings::ApplePhotos(config) => plan_apple_photos(
            &SourcePlanRequest {
                inputs: source.inputs().to_vec(),
                label: source.label().to_owned(),
                config: config.source.clone(),
            },
            cancellation,
            observer,
        ),
        SourceSettings::Picasa(config) => plan_picasa(
            &SourcePlanRequest {
                inputs: source.inputs().to_vec(),
                label: source.label().to_owned(),
                config: config.source.clone(),
            },
            cancellation,
            observer,
        ),
    }
}

pub(super) async fn plan_upload(
    source: &ResolvedSourceProfile,
    client: &ImmichReadClient,
    cancellation: &impl Cancellation,
    observer: &mut impl ApplicationProgressObserver,
) -> Result<UploadPlan, ApplicationError> {
    let root = source
        .inputs()
        .first()
        .cloned()
        .ok_or(ApplicationError::InvalidRequest(
            "source profile has no configured input",
        ))?;
    match source.settings() {
        SourceSettings::Folder(config) => {
            plan_folder_upload(
                &FolderUploadPlanRequest {
                    source: FolderPlanRequest {
                        root,
                        label: source.label().to_owned(),
                        config: config.clone(),
                    },
                },
                client,
                cancellation,
                observer,
            )
            .await
        }
        SourceSettings::GoogleTakeout(config) => {
            plan_google_takeout_upload(
                &SourceUploadPlanRequest {
                    inputs: source.inputs().to_vec(),
                    label: source.label().to_owned(),
                    config: config.clone(),
                },
                client,
                cancellation,
                observer,
            )
            .await
        }
        SourceSettings::ApplePhotos(config) => {
            plan_apple_photos_upload(
                &SourceUploadPlanRequest {
                    inputs: source.inputs().to_vec(),
                    label: source.label().to_owned(),
                    config: config.clone(),
                },
                client,
                cancellation,
                observer,
            )
            .await
        }
        SourceSettings::Picasa(config) => {
            plan_picasa_upload(
                &SourceUploadPlanRequest {
                    inputs: source.inputs().to_vec(),
                    label: source.label().to_owned(),
                    config: config.clone(),
                },
                client,
                cancellation,
                observer,
            )
            .await
        }
    }
}

pub(super) fn dry_run_request(
    source: &ResolvedSourceProfile,
    checkpoint: PathBuf,
) -> UploadDryRunRequest {
    let (config, takeout, apple, picasa) = execution_settings(source.settings());
    UploadDryRunRequest {
        inputs: source.inputs().to_vec(),
        checkpoint,
        config,
        takeout,
        apple,
        picasa,
    }
}

pub(super) fn apply_request(
    source: &ResolvedSourceProfile,
    checkpoint: PathBuf,
) -> UploadApplyRequest {
    let (config, takeout, apple, picasa) = execution_settings(source.settings());
    UploadApplyRequest {
        inputs: source.inputs().to_vec(),
        checkpoint,
        config,
        takeout,
        apple,
        picasa,
    }
}

fn execution_settings(
    settings: &SourceSettings,
) -> (
    UploadExecutionConfig,
    immich_rs_application::TakeoutScanConfig,
    immich_rs_application::ApplePhotosScanConfig,
    immich_rs_application::PicasaScanConfig,
) {
    match settings {
        SourceSettings::Folder(scan) => (
            UploadExecutionConfig {
                scan: scan.clone(),
                ..UploadExecutionConfig::default()
            },
            immich_rs_application::TakeoutScanConfig::default(),
            immich_rs_application::ApplePhotosScanConfig::default(),
            immich_rs_application::PicasaScanConfig::default(),
        ),
        SourceSettings::GoogleTakeout(config) => (
            config.upload.clone(),
            config.source.clone(),
            immich_rs_application::ApplePhotosScanConfig::default(),
            immich_rs_application::PicasaScanConfig::default(),
        ),
        SourceSettings::ApplePhotos(config) => (
            config.upload.clone(),
            immich_rs_application::TakeoutScanConfig::default(),
            config.source.clone(),
            immich_rs_application::PicasaScanConfig::default(),
        ),
        SourceSettings::Picasa(config) => (
            config.upload.clone(),
            immich_rs_application::TakeoutScanConfig::default(),
            immich_rs_application::ApplePhotosScanConfig::default(),
            config.source.clone(),
        ),
    }
}

pub(super) const fn scan_history(kind: SourceKind) -> HistoryKind {
    match kind {
        SourceKind::Folder => HistoryKind::FolderScan,
        SourceKind::GoogleTakeout => HistoryKind::GoogleTakeoutPlan,
        SourceKind::ApplePhotos => HistoryKind::ApplePhotosPlan,
        SourceKind::Picasa => HistoryKind::PicasaPlan,
    }
}

pub(super) const fn plan_history(kind: SourceKind) -> HistoryKind {
    match kind {
        SourceKind::Folder => HistoryKind::FolderPlan,
        SourceKind::GoogleTakeout => HistoryKind::GoogleTakeoutPlan,
        SourceKind::ApplePhotos => HistoryKind::ApplePhotosPlan,
        SourceKind::Picasa => HistoryKind::PicasaPlan,
    }
}

pub(super) const fn dry_run_history(kind: SourceKind) -> HistoryKind {
    match kind {
        SourceKind::Folder => HistoryKind::FolderDryRun,
        SourceKind::GoogleTakeout => HistoryKind::GoogleTakeoutDryRun,
        SourceKind::ApplePhotos => HistoryKind::ApplePhotosDryRun,
        SourceKind::Picasa => HistoryKind::PicasaDryRun,
    }
}

pub(super) const fn apply_history(kind: SourceKind) -> HistoryKind {
    match kind {
        SourceKind::Folder => HistoryKind::FolderApply,
        SourceKind::GoogleTakeout => HistoryKind::GoogleTakeoutApply,
        SourceKind::ApplePhotos => HistoryKind::ApplePhotosApply,
        SourceKind::Picasa => HistoryKind::PicasaApply,
    }
}
