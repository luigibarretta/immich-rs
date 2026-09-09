# Empty Web Console secret-mount example

This directory contains no credentials or key material. It exists only as the
fail-closed default read-only secret mount for Compose configuration checks.
Mount a private operator-owned directory with `IMMICH_RS_WEB_SECRETS_PATH` and
refer to its container paths from the strict Web Console TOML file.
