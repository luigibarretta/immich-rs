ALTER TABLE history RENAME TO history_v1;

CREATE TABLE history (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    recorded_unix INTEGER NOT NULL,
    kind INTEGER NOT NULL CHECK (kind BETWEEN 1 AND 13),
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

INSERT INTO history
    (sequence, recorded_unix, kind, status, plan_ref, plan_schema, assets,
     sidecars, bytes_read, warnings, errors, max_logical_effects)
SELECT sequence, recorded_unix, kind, status, plan_ref, plan_schema, assets,
       sidecars, bytes_read, warnings, errors, max_logical_effects
FROM history_v1;

DROP TABLE history_v1;
