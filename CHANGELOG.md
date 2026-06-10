<!-- markdownlint-disable MD024 -- Keep a Changelog repeats section headings per release -->

# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Tag paths**: `tag = "general/timeOut"` matches against the end of the
  element nesting, so tags that occur in several places can be targeted
  individually.
- **Attribute rules**: `attribute = "debug"` enforces an attribute value
  instead of the element text; a missing attribute is added (unless a
  `forbidden` list scopes the rule to specific wrong values).
- **`ignore_case = true`**: compare values case-insensitively, so
  `true`/`True`/`TRUE` count as equal — convenient for FEWS boolean values.
- **`remove = true`**: forbidden elements (e.g. a `<debugSettings>` block)
  are deleted entirely on fix, including their content and indentation.
- **`required = true`**: the element must be present in the file; a missing
  element fails the hook with a clear message (it cannot be auto-fixed).
- **Empty-element filling**: `<tag/>` where a value is enforced becomes
  `<tag>expected</tag>` (only when the rule has no `forbidden` list).
- **`--well-formed` flag** and the `fews-xml-wellformed` hook id: every
  staged XML file must parse, even when no rule matches it, so syntax errors
  and truncated files are caught.
- Per-file error handling: a broken file no longer aborts the run; remaining
  files are still processed and the hook exits 2 at the end.
- GitHub Actions CI: tests on Linux/Windows/macOS, rustfmt + clippy, and a
  `pre-commit try-repo` integration job that consumes the hook exactly like
  an end user.
- Exploration of XSD schema validation, documented in
  [docs/xsd-validation.md](docs/xsd-validation.md), with a ready-to-use
  sidecar hook configuration based on the Python `xmlschema` package.
- **`fews-xsd-validate` hook**: full XSD schema validation, shipped from
  this repository as a Python hook (pre-commit installs it automatically,
  like the Rust hooks). It resolves the schema *per file* from the
  `xsi:schemaLocation` hint against a local schema directory
  (`--schema-dir`, default `Schemas/`), caches parsed schemas, skips files
  without a schema declaration, and supports `--missing-schema
  warn|skip|fail` plus opt-in `--allow-remote` download. One hook covers
  every FEWS file type — unlike calling `xmlschema-validate` directly,
  which needs one hook per schema. A minimal demonstration schema ships in
  `example/Schemas/`, and CI exercises the valid, invalid and
  missing-schema paths.

### Fixed

- Tail text after a child element is now checked: in
  `<target><note>n</note>False</target>` the `False` was previously
  invisible to value rules.
- Values inside CDATA sections (`<tag><![CDATA[False]]></tag>`) are now
  checked and fixed like plain text; the CDATA wrapper is preserved.
- Removing the *last* child of an element no longer pulls the parent's
  closing tag onto the previous line: the indentation in front of a removed
  element is dropped instead of the whitespace after it.
- A rule with an empty `path` or `tag` is now rejected at config load
  instead of silently never matching.

### Changed

- XML parse errors report the line number instead of a byte offset.
- Attribute violation messages read
  `<export> attribute debug="true", must be "false"` instead of the previous
  comma-spliced form.
- CI cancels superseded runs of the same branch/PR.

### Design choices (also see README "Design choices")

- No full XSD validation inside the Rust binary: the Rust ecosystem offers no
  mature XSD 1.0 validator (the `xmlschema` crate is abandoned at v0.0.1,
  libxml2 bindings would require a native library on every contributor
  machine and break pre-commit's automatic build). Validation is delegated
  to an optional Python sidecar hook instead.
- `remove` cannot be combined with `expected`/`forbidden`/`attribute`/
  `required`; the config is validated up front and a clear error is given.

## [0.1.0] - 2026-06-10

Initial implementation.

### Added

- `fews-precommit` binary: scans XML files passed by pre-commit, reverts
  configured element values to their required defaults, and fails the commit
  so the user can review and re-stage. `--check` reports without modifying.
- Rule configuration in `.fews-precommit.toml` (auto-discovered in the repo
  root, or via `--config`): `path` glob, `tag`, `expected`, optional
  `forbidden` list to revert only known debug values.
- Built-in fallback rule when no config exists:
  `SystemConfigFiles/Explorer.xml` → `<adjustSystemTimeAutomatically>` must
  be `True` (the motivating example).
- `.pre-commit-hooks.yaml` with `fews-xml-defaults` (fix) and
  `fews-xml-defaults-check` (report-only) so any repository can consume the
  hook with three lines of `.pre-commit-config.yaml`.
- Rejection of truncated XML (quick-xml accepts a premature end of file by
  default; the hook tracks element depth and errors instead — found by the
  integration test suite).
- Formatting-preserving rewrite: comments, attributes, whitespace, CRLF line
  endings, UTF-8 BOM and the XML declaration survive byte-for-byte; only the
  violating value changes.
- Test suite: unit tests on the rewrite engine plus end-to-end tests that run
  the compiled binary against temporary FEWS-config-like repositories.
- Annotated example configuration under `example/`.

### Design choices

- **Rust + `language: rust`** so users need no Rust knowledge: pre-commit
  compiles the hook on first use and can even download a toolchain
  (`language_version`).
- **Event-streaming rewrite (quick-xml)** instead of a DOM round-trip (would
  reformat files) or regex (blind to XML structure): minimal diffs in
  reviewed config repositories.
- **Fix-by-default** like rustfmt/black: the hook repairs the file and exits
  non-zero; the user re-stages consciously.
- **No clap dependency**: a hand-rolled parser for the four CLI flags keeps
  the dependency tree small, which matters because every user compiles the
  hook once — and clap's `windows-sys` transitively broke builds on
  MinGW-only Windows setups.
- **Exact value comparison by default**: reverting `true` → `True` is
  intentional (the repository convention wins); `ignore_case` is opt-in.

[Unreleased]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/HKV-products-services/fews-precommit/releases/tag/v0.1.0
