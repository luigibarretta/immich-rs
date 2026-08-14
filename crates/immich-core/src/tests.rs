use super::{
    Cancellation, CancellationToken, CandidateAsset, MediaKind, NORMALIZED_PLAN_SCHEMA_VERSION,
    NeverCancel, NormalizedPlan, PlanSummary, SourceDescriptor, SourceKind, UnicodeNormalization,
};

fn valid_plan() -> NormalizedPlan {
    NormalizedPlan {
        schema_version: NORMALIZED_PLAN_SCHEMA_VERSION,
        source: SourceDescriptor {
            kind: SourceKind::Folder,
            label: "synthetic-basic".to_owned(),
            fingerprint_sha256: "a".repeat(64),
            case_sensitive: true,
            unicode_normalization: UnicodeNormalization::Nfc,
        },
        assets: vec![CandidateAsset {
            operation_id: "b".repeat(64),
            relative_path: "image.jpg".to_owned(),
            media_kind: MediaKind::Image,
            byte_len: 4,
            content_sha256: "c".repeat(64),
            metadata: Vec::new(),
            live_photo: None,
            evidence: Vec::new(),
        }],
        warnings: Vec::new(),
        errors: Vec::new(),
        summary: PlanSummary {
            assets: 1,
            sidecars: 0,
            bytes_read: 4,
        },
    }
}

#[test]
fn normalized_plan_round_trips_without_losing_schema_facts()
-> Result<(), Box<dyn std::error::Error>> {
    let plan = valid_plan();
    let encoded = serde_json::to_string(&plan)?;
    let decoded: NormalizedPlan = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, plan);
    decoded.validate()?;
    Ok(())
}

#[test]
fn normalized_plan_rejects_unsorted_assets() {
    let mut plan = valid_plan();
    let mut second = plan.assets[0].clone();
    second.relative_path = "another.jpg".to_owned();
    second.operation_id = "d".repeat(64);
    plan.assets.push(second);
    plan.summary.assets = 2;
    assert!(plan.validate().is_err());
}

#[test]
fn cancellation_is_explicit_and_clone_safe() {
    let token = CancellationToken::default();
    let observer = token.clone();
    assert!(!observer.is_cancelled());
    token.cancel();
    assert!(observer.is_cancelled());
    assert!(!NeverCancel.is_cancelled());
}
