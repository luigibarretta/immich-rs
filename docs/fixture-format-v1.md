# Synthetic fixture format v1

`fixture-manifest-v1` is the only accepted fixture manifest schema. The JSON
Schema is [`tests/fixtures/schema-v1.json`](../tests/fixtures/schema-v1.json).

Every fixture is declarative and has an identifier beginning with
`synthetic-`. Its provenance must name the repository generator, its generator
version and `CC0-1.0`. A manifest lists portable relative output paths in
strictly ascending order and one bounded recipe for every output:

- `text` writes the declared UTF-8 text;
- `synthetic_png` generates a valid RGB PNG from dimensions and one RGB value;
- `synthetic_isobmff` generates a tiny deterministic ISO-BMFF test object;
- `symlink` creates a relative symbolic link for filesystem-policy tests.

Recipes are materialized only into a caller-selected empty directory. Paths
must be Unicode, relative, traversal-free and unique. The format deliberately
has no recipe for importing an existing media file.

Each fixture names one `normalized-plan-v1` expected file and its SHA-256.
Expected plans contain only portable relative paths and stable hashes; absolute
paths, wall-clock timestamps, server IDs and presentation logs are forbidden.

Run these checks after changing fixtures:

```bash
python3 scripts/check-fixtures.py
python3 -m unittest discover -s tests/tooling -p 'test_*.py'
```

A future schema must use a new schema identifier and document deterministic
migration. Existing expected plans remain compatibility evidence.
