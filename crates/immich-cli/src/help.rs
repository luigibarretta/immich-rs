/// Top-level `--help` text, printed after the `immich-rs <VERSION>` banner.
pub const HELP: &str = "\
Bounded source imports, verified archive and disposable migration

Usage:
  immich-rs config show
  immich-rs plan folder [SCAN_OPTIONS] [PATH]
  immich-rs plan google-takeout [TAKEOUT_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan apple-photos [APPLE_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan picasa [PICASA_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan upload folder [SCAN_OPTIONS] [SERVER_OPTIONS] [PATH]
  immich-rs plan upload google-takeout [TAKEOUT_OPTIONS] [SERVER_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan upload apple-photos [APPLE_OPTIONS] [SERVER_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan upload picasa [PICASA_OPTIONS] [SERVER_OPTIONS] [DIRECTORY|ZIP...]
  immich-rs plan archive immich [ARCHIVE_PLAN_OPTIONS]
  immich-rs plan migration immich [MIGRATION_PLAN_OPTIONS]
  immich-rs inspect upload-plan --plan <FILE>
  immich-rs apply upload [UPLOAD_OPTIONS]
  immich-rs apply archive [ARCHIVE_APPLY_OPTIONS]
  immich-rs apply migration immich [MIGRATION_APPLY_OPTIONS]
  immich-rs --help | --version

Every command accepts --config <FILE> before the command name.

SCAN_OPTIONS:
  --label <LABEL>                      Source label recorded in the plan
  --buffer-bytes <BYTES>               Read buffer size
  --max-entries <COUNT>                Maximum entries in the whole source
  --max-directory-entries <COUNT>      Maximum entries in one directory
  --max-path-bytes <BYTES>             Maximum relative path length
  --case-sensitive | --case-insensitive

TAKEOUT_OPTIONS (also accept SCAN_OPTIONS for an extracted directory):
  --max-archives <COUNT>               Maximum ZIP parts
  --max-archive-entry-bytes <BYTES>    Maximum uncompressed size of one ZIP entry
  --max-compression-ratio <RATIO>      Compression-bomb guard
  --compression-ratio-grace-bytes <BYTES>

APPLE_OPTIONS (TAKEOUT_OPTIONS plus):
  --album-mode <none|folder|path>      How folders become albums
  --album-path-joiner <TEXT>           Separator for --album-mode path

PICASA_OPTIONS (APPLE_OPTIONS plus):
  --picasa-albums | --no-picasa-albums Read .picasa.ini album membership
  --filename-date | --no-filename-date Derive capture dates from file names

SERVER_OPTIONS (read-only access to the destination):
  --server <HTTPS_URL>                 Destination Immich server
  --authorize-production-read          Required for every non-disposable server
  --ca-certificate <PEM_FILE>          Private CA only; hostnames are always verified

UPLOAD_OPTIONS (also accept the source options used to create the plan):
  --plan <FILE>                        Reviewed upload plan
  --source <PATH> | --input <PATH>...  Same source the plan was created from
  --checkpoint <FILE>                  SQLite checkpoint for resumable apply
  --dry-run | --no-dry-run             Verify without mutating the server
  --server <HTTPS_URL>  --ca-certificate <PEM_FILE>
  --verification-buffer-bytes <BYTES>  --concurrency <COUNT>
  --max-attempts-per-operation <COUNT> --max-retries-per-run <COUNT>
  --retry-base-delay-ms <MS>           --retry-delay-cap-ms <MS>
  Production writes additionally require all of:
  --authorize-production-read
  --authorize-production-write
  --confirm-plan-sha256 <HEX>          Digest printed by `inspect upload-plan`
  --expected-operations <COUNT>        Operation count printed by `inspect upload-plan`
  --backup-reference <TEXT>            Restore point of the destination server

  The backup reference is free text naming a restore point that the Immich
  administrator created and verified, for example a database dump path and a
  filesystem snapshot name. immich-rs never contacts or checks that backup and
  records only its SHA-256, so the option needs no server administration
  rights: operators who do not administer the server ask the administrator
  for the reference. It must be non-empty, without leading or trailing
  whitespace or control characters.

ARCHIVE_PLAN_OPTIONS:
  --server <HTTPS_URL>  --authorize-production-read  --ca-certificate <PEM_FILE>
  --selection <timeline|archive|hidden|all>
  --include-trashed | --exclude-trashed
  --page-size <COUNT>  --max-assets <COUNT>

ARCHIVE_APPLY_OPTIONS:
  --manifest <FILE>  --destination <DIRECTORY>
  --server <HTTPS_URL>  --authorize-production-read  --ca-certificate <PEM_FILE>

MIGRATION_PLAN_OPTIONS (disposable servers only):
  --source-server <URL>  --destination-server <URL>
  --page-size <COUNT>  --max-assets <COUNT>  --max-albums <COUNT>
  --max-album-memberships <COUNT>  --max-asset-bytes <BYTES>  --max-total-bytes <BYTES>

MIGRATION_APPLY_OPTIONS (disposable servers only):
  --plan <FILE>  --checkpoint <FILE>  --source-server <URL>  --destination-server <URL>
  --dry-run | --no-dry-run

Production authorization flags are CLI-only and cannot be set in TOML or the environment.
Format and executor resource limits are documented in docs/configuration.md.
Boolean options have explicit positive and negative CLI forms where inheritance matters.
Other options can use CLI, IMMICH_RS_* environment or strict schema-v1 TOML configuration.
API keys use IMMICH_RS_API_KEY or IMMICH_RS_API_KEY_FILE and are never accepted in TOML.
No delete, replace, trash or independent metadata-mutation command exists.";

#[cfg(test)]
mod tests {
    use super::HELP;

    /// Every option accepted by an argument parser must be documented in `--help`.
    #[test]
    fn help_documents_every_parsed_option() {
        let sources = [
            include_str!("apply_args.rs"),
            include_str!("import_plan_args.rs"),
            include_str!("inspect.rs"),
            include_str!("migration_apply_args.rs"),
            include_str!("migration_plan_args.rs"),
            include_str!("picasa_plan_args.rs"),
            include_str!("plan_args.rs"),
        ];
        let mut missing = Vec::new();
        for source in sources {
            for (at, _) in source.match_indices("Some(\"--") {
                let option = &source[at + "Some(\"".len()..];
                let Some(end) = option.find('"') else {
                    continue;
                };
                let option = &option[..end];
                let documented = HELP
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .any(|word| word == option);
                if !documented && !missing.contains(&option) {
                    missing.push(option);
                }
            }
        }
        assert!(missing.is_empty(), "undocumented options: {missing:?}");
    }

    #[test]
    fn backup_reference_explains_who_provides_it() {
        assert!(HELP.contains("--backup-reference <TEXT>"));
        assert!(HELP.contains("needs no server administration"));
        assert!(HELP.contains("never contacts or checks that backup"));
    }
}
