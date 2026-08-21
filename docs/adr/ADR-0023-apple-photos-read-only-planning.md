# ADR-0023: Apple Photos export read-only planning

- Status: Accepted
- Date: 2026-08-21
- Owners: project maintainers

## Context

Phase 4 adds Apple Photos and iCloud export discovery without authorizing an
upload. Apple documents both rendered exports and unmodified-original exports,
optional XMP sidecars, separate still and video files for Live Photos, and ZIP
downloads from iCloud.com. An exported tree does not provide a universal,
machine-verifiable relationship between rendered edits and originals. Guessing
that relationship from filenames could silently discard a valid asset.

immich-go v0.32.0 remains a pinned black-box oracle. Its observable discovery
behavior can define compatibility evidence, but its source code is not an
implementation input and its dry-run mutations are not copied.

## Decision

### Source and schema boundary

`plan apple-photos` accepts either one real directory or one to 64 independent
ZIP files. Directory and ZIP inputs cannot be mixed. ZIP paths are virtual and
are never extracted. Stored and DEFLATE entries are streamed through bounded
buffers; traversal, encryption, symlinks, unsupported compression, invalid
CRC, excessive entry size and suspicious compression ratios fail closed.

Apple plans use `normalized-plan-v3` and `SourceKind::ApplePhotos`. Versions 1
and 2 remain immutable. Version 3 reuses the source-neutral normalized metadata
contract and is valid only for Apple Photos inputs.

Known export noise is skipped by portable path rule before its bytes are read:
AppleDouble files, `.DS_Store`, `@eaDir`, `.Spotlight-V100`,
`.photostructure`, thumbnail database files and `Recently Deleted` trees. Each
skip is an explainable bounded diagnostic. Hidden paths not in this list are
not guessed to be noise.

### Asset, metadata and album policy

Every distinct supported media path is preserved. The planner never infers
that a file is an edited derivative or chooses an original from a filename.
Content-identical paths also remain distinct because folder placement can carry
explicit album meaning. This preserve-all policy is deterministic and avoids
silent loss; a future relation model requires a superseding ADR and new schema.

XMP is associated only by the existing exact or unambiguous same-directory
basename rules. JSON is treated as a generic sidecar, not Apple Photos
metadata. A same-directory image and MOV with one shared stem form one Live
Photo pair; ambiguous groups remain separate. All decisions carry stable rule
IDs.

Album mapping is explicit: `none` adds no album, `folder` adds the immediate
parent name, and `path` adds the full parent path joined by a caller-selected
bounded separator. The default is `none`, matching the absence of an implicit
album request. Root-level files have no derived album. Album strings are NFC,
bounded, sorted and deduplicated.

### Evidence and capability boundary

Synthetic directory and split-ZIP views define the exact golden plan. Property
tests permute ZIP input and entry order, buffer size and album mode. The pinned
oracle runs only against the loopback mock with synthetic media. Differential
rows compare discovered assets, XMP, Live Photos, folder albums, Unicode and
dry-run mutation observations; unavoidable oracle behavior is classified.

The Apple adapter belongs only to `immich-sources` and `immich-core`. It cannot
depend on the Immich client or executor. No Apple plan is accepted by the
folder upload planner or executor.

## Consequences

Phase 4 supports documented Apple export shapes with bounded memory and no
server capability. It deliberately does not recreate Photos library edits,
faces, memories, Shared Album comments or likes. Users who need an original
only must export unmodified originals at the source; immich-rs will not guess.

ZIP central-directory metadata remains bounded by the entry cap while payloads
are streamed. Multiple independent iCloud ZIP downloads may be scanned as one
logical input, with identical duplicate logical paths coalesced and conflicts
reported as errors.

## Verification

Unit tests cover layout validation, noise filtering before reads, sidecars,
Live Photos, preserve-all variants, album modes, Unicode, duplicate/conflicting
ZIP paths, archive safety, cancellation and deterministic ordering. Exact
directory/ZIP goldens and the pinned black-box differential run in push CI.
A separate manual paired benchmark records raw metrics and the complete
environment/fixture manifest before any performance statement is published.
