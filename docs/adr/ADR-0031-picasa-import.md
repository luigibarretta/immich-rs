# ADR-0031: Picasa import

- Status: Accepted
- Date: 2026-08-24
- Owners: project maintainers

## Context

The pinned immich-go v0.32.0 black-box exposes `upload from-picasa` for a
folder or ZIP source. immich-rs already has bounded local discovery, XMP and
Live Photo reconciliation, immutable source-aware upload plans and
checkpointed import effects, but it has no Picasa-specific metadata adapter.

Picasa exports may contain `.picasa.ini` files whose album and per-file fields
must not be treated as arbitrary configuration. Real personal exports are not
yet available, so implementation and external compatibility evidence must be
reported separately.

## Decision

Add a distinct `Picasa` source kind and versioned normalized plan. The accepted
initial compatibility surface is:

1. one directory or a bounded set of independent ZIP inputs;
2. deterministic recursive discovery with the existing media, XMP, Unicode,
   collision, symlink, cancellation and source-change rules;
3. bounded UTF-8 `.picasa.ini` parsing for a `[Picasa]` album name and
   per-file caption only;
4. explicit `none`, `folder` and `path` folder-album modes, with the Picasa
   album enabled by default and a bounded joiner;
5. an optional deterministic filename-date fallback that never overrides a
   stronger normalized timestamp;
6. immutable `upload-plan-v2`, offline source-aware dry-run, one-entry ZIP
   staging and effect-level `checkpoint-v2` apply;
7. exact source/configuration/server/mutation-budget/backup authorization for
   remote HTTPS writes.

Unknown INI sections and keys are ignored as unsupported source metadata and
cannot create tags, people, favorites, stacks or arbitrary server mutations.
The parser has explicit file, line, section, key and value limits. Invalid
encoding, duplicate conflicting assignments and unsafe names fail closed with
rule IDs.

The implementation gate uses only synthetic fixtures. The black-box
`from-picasa` differential, comparable benchmark and authorized personal-export
shadow remain separate required evidence. No Picasa performance or production
claim is allowed before those gates pass.

## Consequences

- Picasa behavior cannot be silently folded into folder scanning.
- The default surface preserves albums and captions without authorizing tags or
  destructive behavior.
- Future Picasa fields require a new ADR and compatibility evidence.
- No Go source may be inspected, copied, translated or linked.

## Verification

Unit, golden and property tests cover bounded INI parsing, deterministic
directory/ZIP equivalence, captions, album modes, filename-date precedence,
collisions, cancellation and source drift. Mock transport tests cover offline
dry-run plus bounded authentication, compatibility, retry, disconnect and
lost-response behavior for every authorized effect.

A disposable synthetic gate records aggregate postconditions, raw benchmark
measurements and exact resource cleanup. Black-box immich-go v0.32.0 results
and an authorized personal-export shadow are recorded as separate evidence;
the Picasa compatibility gate remains incomplete until both are committed and
green for the implementation SHA.
