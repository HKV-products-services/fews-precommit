<!-- markdownlint-disable MD024 -- Keep a Changelog repeats section headings per release -->

# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `fews-config-validate --test-run-all`: generates and runs a workflow
  test run containing every workflow from `WorkflowDescriptors.xml`,
  scopeable with `--include`/`--exclude` regexes — the deepest
  command-line check available, resolving module configs, time series
  sets and (time-dependent) related locations by executing the workflows.
  Found a real nested-LocationSet error on first use against a production
  configuration. Supporting options: `--system-time` (pins the test run
  system time) and `--region-copy` (copies e.g. `Modules` into the
  throwaway region for workflows that run adapters).
- README: mapping of the hook modes to the three FEWS validation levels
  (startup log, Workflow Navigator, workflow run). The Workflow Navigator
  (F12 > K) is GUI-only in Delft-FEWS 2025.02 — there is no command-line
  equivalent and `mctools` has no config check tool; a dedicated
  `configchecktool` would be a feature request to Deltares.

### Fixed

- `fews-config-validate`: multi-line log capture no longer glues log
  banners (`*** WorkflowTestRun Start ***`) onto error messages, which
  also broke deduplication.

## [0.1.4] - 2026-06-12

### Added

- **`fews-config-validate` hook**: lets Delft-FEWS itself load the
  configuration and fails on every `ERROR`/`FATAL` it logs — catching
  semantic errors XSD validation cannot see (broken references between
  files, unknown location/grid ids, …). It builds a throwaway region whose
  `Config` links to the repository's configuration and runs the Delft-FEWS
  Data Conversion module through the user's `Delft-FEWSc.exe`; the
  executable path is machine-specific and user-defined via the `FEWS_EXE`
  environment variable or `--fews-exe`. With `--test-run
  <workflowTestRun.xml>` it runs a Delft-FEWS *workflow test run* instead
  (main class `nl.wldelft.fews.system.workflowtestrun.WorkflowTestRun`),
  executing workflows against the candidate configuration. The region
  folder and `*patch*.jar` are auto-detected; `--workflow`,
  `--global-properties` and `--max-memory` (default 1024 MB) tune the run.
  Wired to the pre-push and manual stages (~30s per run). Note: the Data
  Conversion module exits 0 even for a broken configuration, so the hook
  parses the run log instead of trusting the exit code; multi-line log
  entries are captured in full.

## [0.1.3] - 2026-06-11

### Changed

- **`fews-xsd-validate` is now 10–20× faster**: validation runs on lxml
  (libxml2, the C library behind xmllint) instead of the pure-Python
  xmlschema engine. Measured on a real FEWS configuration (325 files,
  full schema set): 1.9s instead of 26.7s. Validation errors now include
  line numbers. Schemas that strict libxml2 rejects — such as shipped FEWS
  schemas containing a schemaLocation with a stray trailing space
  (`taskProperties.xsd` → `"mcSynchronisation.xsd "`) — are handled
  transparently by the tolerant pure-Python engine as a fallback, so no
  file goes unvalidated. Java/Xerces was considered and rejected: similar
  speed, but it would require a JRE on every machine and pre-commit has no
  first-class Java support.

## [0.1.2] - 2026-06-11

### Added

- `fews-xsd-validate`: relative `xsi:schemaLocation` hints (e.g.
  `../xsd/rtcDataConfig.xsd`, common in RTC-Tools configs) are now resolved
  against the XML file's own location, also when written with Windows
  backslashes. Previously such files could only be validated by copying the
  schema into the schema directory, and `--allow-remote` appeared to have
  no effect on them.

### Changed

- `fews-xsd-validate`: validation errors are now printed structured and
  width-wrapped (file plus error count, then indented element path and
  reason) instead of one long line per error.
- `fews-xsd-validate`: the missing-schema message no longer suggests
  `--allow-remote` when it cannot help (flag already set, or the hint is
  not an http(s) URL).

## [0.1.1] - 2026-06-11

### Changed

- **The built-in default rule and all examples now enforce lowercase
  `true`** for `adjustSystemTimeAutomatically`. A field test on a real FEWS
  configuration showed the capitalized `True` from the original plan is not
  what FEWS uses: the schemas declare `xs:boolean`, which only allows
  `true`/`false`/`1`/`0`, so enforcing `True` would corrupt a valid file.

### Added

- README troubleshooting section: the "linker `link.exe` not found" error
  on Windows machines without Visual Studio Build Tools (fix: switch
  rustup to the GNU toolchain), and the one-time compilation wait on first
  use.

## [0.1.0] - 2026-06-10

Initial release.

### Added

- `fews-precommit` binary: scans XML files passed by pre-commit, reverts
  configured element values to their required defaults, and fails the commit
  so the user can review and re-stage. `--check` reports without modifying.
- Rule configuration in `.fews-precommit.toml` (auto-discovered in the repo
  root, or via `--config`): `path` glob, `tag`, `expected`, optional
  `forbidden` list to revert only known debug values.
- **Tag paths**: `tag = "general/timeOut"` matches against the end of the
  element nesting, so tags that occur in several places can be targeted
  individually.
- **Attribute rules**: `attribute = "debug"` enforces an attribute value
  instead of the element text; a missing attribute is added (unless a
  `forbidden` list scopes the rule to specific wrong values).
- **`ignore_case = true`**: compare values case-insensitively, so
  `true`/`True`/`TRUE` count as equal.
- **`remove = true`**: forbidden elements (e.g. a `<debugSettings>` block)
  are deleted entirely on fix, including their content and indentation.
- **`required = true`**: the element must be present in the file; a missing
  element fails the hook with a clear message (it cannot be auto-fixed).
- **Empty-element filling**: `<tag/>` where a value is enforced becomes
  `<tag>expected</tag>` (only when the rule has no `forbidden` list).
- **`--well-formed` flag** and the `fews-xml-wellformed` hook id: every
  staged XML file must parse, even when no rule matches it, so syntax errors
  and truncated files are caught. Truncated XML is always rejected for
  matched files (quick-xml accepts a premature end of file by default; the
  hook tracks element depth instead).
- **`fews-xsd-validate` hook**: full XSD schema validation, shipped from
  this repository as a Python hook (pre-commit installs it automatically,
  like the Rust hooks). It resolves the schema *per file* from the
  `xsi:schemaLocation` hint against a local schema directory
  (`--schema-dir`, default `Schemas/`), caches parsed schemas, skips files
  without a schema declaration, and supports `--missing-schema
  warn|skip|fail` plus opt-in `--allow-remote` download. One hook covers
  every FEWS file type. The exploration behind it is documented in
  [docs/xsd-validation.md](docs/xsd-validation.md).
- Built-in fallback rule when no config exists:
  `SystemConfigFiles/Explorer.xml` → `<adjustSystemTimeAutomatically>`.
- `.pre-commit-hooks.yaml` exposing `fews-xml-defaults` (fix),
  `fews-xml-defaults-check` (report-only), `fews-xml-wellformed` and
  `fews-xsd-validate`, so any repository can consume the hooks with a few
  lines of `.pre-commit-config.yaml`.
- Formatting-preserving rewrite: comments, attributes, whitespace, CRLF line
  endings, UTF-8 BOM, CDATA sections and the XML declaration survive
  byte-for-byte; only the violating value changes. Values in CDATA and in
  tail text after child elements are checked too.
- Per-file error handling: a broken file does not abort the run; remaining
  files are still processed and the hook exits 2 at the end. XML parse
  errors report line numbers.
- Test suite: unit tests on the rewrite engine plus end-to-end tests that
  run the compiled binary against temporary FEWS-config-like repositories.
- GitHub Actions CI: tests on Linux/Windows/macOS, rustfmt + clippy, an XSD
  hook job, and a `pre-commit try-repo` integration job that consumes the
  hooks exactly like an end user. Superseded runs are cancelled.
- Annotated example configuration under `example/`, including a
  demonstration schema for the XSD hook.

### Design choices

- **Rust + `language: rust`** so users need no Rust knowledge: pre-commit
  compiles the hook on first use and can even download a toolchain
  (`language_version`).
- **Event-streaming rewrite (quick-xml)** instead of a DOM round-trip (would
  reformat files) or regex (blind to XML structure): minimal diffs in
  reviewed config repositories.
- **Fix-by-default** like rustfmt/black: the hook repairs the file and exits
  non-zero; the user re-stages consciously.
- **No clap dependency**: a hand-rolled parser for the few CLI flags keeps
  the dependency tree small, which matters because every user compiles the
  hook once — and clap's `windows-sys` transitively broke builds on
  MinGW-only Windows setups.
- **Exact value comparison by default**: the repository convention wins;
  `ignore_case` is opt-in.
- **No full XSD validation inside the Rust binary**: the Rust ecosystem
  offers no mature XSD 1.0 validator (the `xmlschema` crate is abandoned,
  libxml2 bindings would require a native library on every contributor
  machine). Validation is delegated to the Python sidecar hook instead.

[Unreleased]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/HKV-products-services/fews-precommit/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/HKV-products-services/fews-precommit/releases/tag/v0.1.0
