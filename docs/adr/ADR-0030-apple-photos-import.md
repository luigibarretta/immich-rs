# ADR-0030: Apple Photos import

- Status: Accepted
- Date: 2026-08-24
- Owners: project maintainers
- Supersedes: ADR-0023

## Context

ADR-0023 completed deterministic read-only Apple Photos and iCloud export
planning while explicitly preventing its normalized plan from entering an
executor. ADR-0028 subsequently proved a source-aware, bounded and idempotent
import capability for Google Takeout. Apple imports need the same immutable
plan, staging, checkpoint and production-authorization guarantees without
guessing relationships that an export does not prove.

Apple exports may be a real directory or independent split ZIP files. They can
contain XMP sidecars, separate image and MOV Live Photo members, rendered and
unmodified variants, and folder placement chosen by the exporter. None of
those facts authorizes destructive replacement or collapsing distinct paths.

## Decision

### Retained source contract

`plan apple-photos` retains the complete ADR-0023 contract: one real directory
or one to 64 independent ZIP parts, bounded streaming without global
extraction, preserve-all media paths, exact or unambiguous XMP association,
same-directory Live Photo pairing, known-noise diagnostics, Unicode NFC and
explicit `none`, `folder` or `path` album derivation. The source-only command
still cannot construct an Immich client.

`normalized-plan-v3` remains immutable and Apple-only. The resolved scanner
additionally retains ephemeral directory or ZIP-entry locators, base64 SHA-1
and bounded source timestamp facts for immediate plan construction. Those
native locators never enter normalized output.

### Import plan and execution

`plan upload apple-photos` emits `upload-plan-v2`. It binds the exact
`normalized-plan-v3` digest, source fingerprint, Apple scan configuration,
server compatibility, media/XMP operations, Live Photo dependencies, derived
albums and the maximum mutation budget. Directory and split-ZIP views of the
same logical source must produce equivalent operations and summaries.

The existing `apply upload` command selects its source adapter from the plan,
not from an operator-supplied format flag. Dry-run rescans and verifies the
complete Apple input offline without reading a credential, creating a
checkpoint or constructing a network capability. Apply stages at most one ZIP
media entry and optional XMP sidecar, verifies length and both content
identities, and removes exact plan-owned staging on every outcome.

Transport timestamps use the source filesystem timestamps for directory
inputs and the bounded ZIP entry timestamp for archives when no resolved
capture instant exists. They do not infer edit/original relationships. Embedded
media metadata and XMP remain available to Immich's normal extraction path.

Asset upload, album create and album membership are durable independent effects
in `checkpoint-v2`. Exact album lookup, duplicate convergence, lost-response
reconciliation, bounded retry and cancellation retain ADR-0028 behavior.
Existing albums are reused but never renamed or deleted. JSON sidecars remain
generic evidence and do not authorize an Apple-specific metadata mutation.

### Authorization and evidence

Disposable loopback apply uses only a synthetic owner and media. Remote HTTPS
apply retains ADR-0027's CLI-only read/write acknowledgements and binds the
complete plan digest, maximum mutation count and hashed verified-backup
reference. The required API permissions are limited to asset upload/read and,
when albums exist, album read/create plus album membership.

The compatibility gate uses the exact immich-go v0.32.0 executable only as a
black-box `upload from-icloud` oracle. It covers directory and split ZIP input,
XMP, Live Photos, preserve-all paths, album derivation, offline dry-run, first
apply, checkpoint resume, fresh-checkpoint duplicate convergence, fault
injection and complete disposable cleanup. Paired performance evidence uses
the same synthetic corpus, server, environment and concurrency before any
performance statement.

## Consequences

Apple exports gain the same bounded, resumable import path as Google Takeout
without weakening the source-only planner. Folder-derived albums can create
additional server effects, all visible in the immutable mutation budget.

Photos edits, faces, people, Shared Album comments, likes, Memories and
unproven original/edited relationships remain unsupported. Delete, replace,
trash, tag, stack and independent maintenance mutations remain absent.

## Verification

Unit and property tests cover source/configuration binding, directory/ZIP
equivalence, timestamp facts, XMP and Live Photo execution ordering, album
effects, cancellation, staging ownership and schema/source mismatch. Mock tests
cover authentication, version refusal, 429, 5xx, disconnect and lost responses
at every durable effect.

The disposable Immich v3.1.0 gate records only synthetic aggregate counters,
postconditions and exact cleanup. Push CI validates the differential matrix,
dependency boundary and evidence. Apple apply is not documented as complete
until its implementation SHA, disposable gate and paired raw benchmark are
committed and green.
