CREATE TABLE history (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    recorded_unix INTEGER NOT NULL,
    kind INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 5),
    status INTEGER NOT NULL CHECK (status BETWEEN 1 AND 3),
    plan_ref BLOB CHECK (plan_ref IS NULL OR length(plan_ref) = 16),
    plan_schema INTEGER CHECK (plan_schema IS NULL OR plan_schema > 0),
    assets INTEGER NOT NULL CHECK (assets >= 0),
    sidecars INTEGER NOT NULL CHECK (sidecars >= 0),
    bytes_read INTEGER NOT NULL CHECK (bytes_read >= 0),
    warnings INTEGER NOT NULL CHECK (warnings >= 0),
    errors INTEGER NOT NULL CHECK (errors >= 0),
    max_logical_effects INTEGER NOT NULL CHECK (max_logical_effects >= 0),
    CHECK ((plan_ref IS NULL) = (plan_schema IS NULL))
) STRICT;

CREATE TABLE dry_run_receipts (
    receipt_ref BLOB PRIMARY KEY CHECK (length(receipt_ref) = 16),
    plan_ref BLOB NOT NULL CHECK (length(plan_ref) = 16),
    plan_sha256 BLOB NOT NULL CHECK (length(plan_sha256) = 32),
    source_configuration_sha256 BLOB NOT NULL
        CHECK (length(source_configuration_sha256) = 32),
    server_identity_sha256 BLOB NOT NULL CHECK (length(server_identity_sha256) = 32),
    server_profile_sha256 BLOB NOT NULL CHECK (length(server_profile_sha256) = 32),
    credential_generation INTEGER NOT NULL CHECK (credential_generation > 0),
    max_logical_effects INTEGER NOT NULL CHECK (max_logical_effects >= 0),
    completed_unix INTEGER NOT NULL
) STRICT;

CREATE INDEX dry_run_receipts_completed_idx
    ON dry_run_receipts(completed_unix);
