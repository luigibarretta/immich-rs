//! Stable rule identifiers shared by scanners, plans and diagnostics.

/// A regular media file was accepted after a bounded streaming read.
pub const REGULAR_MEDIA: &str = "FS_REGULAR_MEDIA_V1";
/// A sidecar was associated by an exact relative filename.
pub const SIDECAR_EXACT_NAME: &str = "META_SIDECAR_EXACT_NAME_V1";
/// A sidecar was associated by an unambiguous basename.
pub const SIDECAR_BASENAME: &str = "META_SIDECAR_BASENAME_V1";
/// A Google Takeout JSON title selected one same-directory media file.
pub const GOOGLE_TAKEOUT_TITLE: &str = "META_GOOGLE_TAKEOUT_TITLE_V1";
/// A Google Takeout JSON sidecar was malformed or had an invalid title.
pub const GOOGLE_TAKEOUT_JSON_INVALID: &str = "META_GOOGLE_TAKEOUT_JSON_INVALID_V1";
/// A Google Takeout JSON sidecar exceeded the bounded parser limit.
pub const GOOGLE_TAKEOUT_JSON_OVERSIZED: &str = "META_GOOGLE_TAKEOUT_JSON_OVERSIZED_V1";
/// Multiple Google Takeout JSON sidecars selected the same media file.
pub const GOOGLE_TAKEOUT_AMBIGUOUS: &str = "META_GOOGLE_TAKEOUT_AMBIGUOUS_V1";
/// A metadata family is outside the first Google Takeout vertical.
pub const GOOGLE_TAKEOUT_UNSUPPORTED: &str = "META_GOOGLE_TAKEOUT_UNSUPPORTED_V1";
/// A Google Takeout media file had no associated JSON metadata candidate.
pub const GOOGLE_TAKEOUT_UNMATCHED_MEDIA: &str = "META_GOOGLE_TAKEOUT_UNMATCHED_MEDIA_V1";
/// A supplemental JSON basename selected one same-directory media file.
pub const GOOGLE_TAKEOUT_SUPPLEMENTAL: &str = "META_GOOGLE_TAKEOUT_SUPPLEMENTAL_V1";
/// Album metadata contributed a bounded album title to an asset.
pub const GOOGLE_TAKEOUT_ALBUM: &str = "META_GOOGLE_TAKEOUT_ALBUM_V1";
/// Conflicting normalized metadata was rejected without choosing a winner.
pub const GOOGLE_TAKEOUT_METADATA_CONFLICT: &str = "META_GOOGLE_TAKEOUT_CONFLICT_V1";
/// Content-identical Takeout paths were collapsed into one logical asset.
pub const GOOGLE_TAKEOUT_CONTENT_ALIAS: &str = "META_GOOGLE_TAKEOUT_CONTENT_ALIAS_V1";
/// Identical logical entries across independent Takeout archives were coalesced.
pub const TAKEOUT_ARCHIVE_DUPLICATE: &str = "ARCHIVE_TAKEOUT_DUPLICATE_IDENTICAL_V1";
/// Independent Takeout archives contain conflicting bytes at one logical path.
pub const TAKEOUT_ARCHIVE_CONFLICT: &str = "ARCHIVE_TAKEOUT_PATH_CONFLICT_V1";
/// Known non-media Apple export noise was skipped before reading its payload.
pub const APPLE_EXPORT_NOISE: &str = "APPLE_EXPORT_NOISE_SKIPPED_V1";
/// A folder path explicitly contributed Apple export album membership.
pub const APPLE_FOLDER_ALBUM: &str = "META_APPLE_FOLDER_ALBUM_V1";
/// Identical Apple ZIP entries at one logical path were coalesced.
pub const APPLE_ARCHIVE_DUPLICATE: &str = "ARCHIVE_APPLE_DUPLICATE_IDENTICAL_V1";
/// Apple ZIP parts contain conflicting bytes at one logical path.
pub const APPLE_ARCHIVE_CONFLICT: &str = "ARCHIVE_APPLE_PATH_CONFLICT_V1";
/// A bounded Picasa INI caption contributed normalized metadata.
pub const PICASA_CAPTION: &str = "META_PICASA_CAPTION_V1";
/// A bounded Picasa INI album contributed membership.
pub const PICASA_ALBUM: &str = "META_PICASA_ALBUM_V1";
/// Folder placement explicitly contributed Picasa album membership.
pub const PICASA_FOLDER_ALBUM: &str = "META_PICASA_FOLDER_ALBUM_V1";
/// A deterministic timestamp was parsed from a supported Picasa filename prefix.
pub const PICASA_FILENAME_DATE: &str = "META_PICASA_FILENAME_DATE_V1";
/// A Picasa INI document was malformed, conflicting or outside its limits.
pub const PICASA_INI_INVALID: &str = "META_PICASA_INI_INVALID_V1";
/// Identical Picasa ZIP entries at one logical path were coalesced.
pub const PICASA_ARCHIVE_DUPLICATE: &str = "ARCHIVE_PICASA_DUPLICATE_IDENTICAL_V1";
/// Picasa ZIP parts contain conflicting bytes at one logical path.
pub const PICASA_ARCHIVE_CONFLICT: &str = "ARCHIVE_PICASA_PATH_CONFLICT_V1";
/// A still image and video were associated as a live-photo pair.
pub const LIVE_PHOTO_BASENAME: &str = "PAIR_LIVE_PHOTO_BASENAME_V1";
/// A symbolic link was deliberately not followed.
pub const SYMLINK_SKIPPED: &str = "FS_SYMLINK_SKIPPED_V1";
/// A path is not valid Unicode and cannot enter the portable plan.
pub const NON_UNICODE_PATH: &str = "FS_NON_UNICODE_PATH_V1";
/// Two native paths normalize to the same NFC portable path.
pub const UNICODE_COLLISION: &str = "FS_UNICODE_COLLISION_V1";
/// A portable path exceeded the configured byte limit.
pub const PATH_LIMIT_EXCEEDED: &str = "FS_PATH_LIMIT_EXCEEDED_V1";
/// Two portable paths differ only by case.
pub const CASE_COLLISION: &str = "FS_CASE_COLLISION_V1";
/// Multiple assets share a basename in distinct directories.
pub const DUPLICATE_BASENAME: &str = "FS_DUPLICATE_BASENAME_V1";
/// A sidecar could not be associated without ambiguity.
pub const AMBIGUOUS_SIDECAR: &str = "META_AMBIGUOUS_SIDECAR_V1";
/// A sidecar has no supported media candidate.
pub const ORPHAN_SIDECAR: &str = "META_ORPHAN_SIDECAR_V1";
/// A file could not be opened or read.
pub const UNREADABLE_FILE: &str = "FS_UNREADABLE_FILE_V1";
/// File identity changed while content was being read.
pub const SOURCE_CHANGED: &str = "FS_SOURCE_CHANGED_V1";
/// A non-file filesystem entry was deliberately ignored.
pub const SPECIAL_FILE_SKIPPED: &str = "FS_SPECIAL_FILE_SKIPPED_V1";
