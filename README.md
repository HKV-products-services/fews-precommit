# fews-precommit

> [!WARNING]
> This repository was completely created with AI.

A [pre-commit](https://pre-commit.com) hook for Delft-FEWS configuration
repositories. It scans staged XML files and reverts values that were changed
for debugging or testing back to their required defaults, so they never end
up in a commit.

The classic example: while debugging it is convenient to set

```xml
<adjustSystemTimeAutomatically>false</adjustSystemTimeAutomatically>
```

in `SystemConfigFiles/Explorer.xml`, but in the repository this must always be

```xml
<adjustSystemTimeAutomatically>true</adjustSystemTimeAutomatically>
```

This hook reverts it automatically on commit. (FEWS booleans are XSD
`xs:boolean`, so the enforced values are lowercase `true`/`false`.)

## Usage

You only need `git`, `python` and [pre-commit](https://pre-commit.com)
installed — no Rust knowledge required. Add this to the
`.pre-commit-config.yaml` of your FEWS configuration repository:

```yaml
repos:
  - repo: https://github.com/HKV-products-services/fews-precommit
    rev: v0.1.4
    hooks:
      - id: fews-xml-defaults
```

then run:

```sh
pre-commit install
```

pre-commit builds the hook with Cargo on first use. If you do not have a Rust
toolchain installed, let pre-commit download one automatically by pinning a
toolchain version:

```yaml
      - id: fews-xml-defaults
        language_version: "1.83"
```

From now on, every commit that touches a matched XML file is checked. When a
value deviates, the hook reverts it and fails the commit so you can review
and re-stage:

```text
Delft-FEWS XML defaults (fix)............................................Failed
- files were modified by this hook

SystemConfigFiles/Explorer.xml: <adjustSystemTimeAutomatically> is 'false', must be 'true' -- reverted
```

### Available hooks

| Hook id | What it does |
| --- | --- |
| `fews-xml-defaults` | Reverts violating values in place and fails the commit (re-stage to proceed). |
| `fews-xml-defaults-check` | Reports violations without modifying files. |
| `fews-xml-wellformed` | Additionally fails when any staged XML file has syntax errors or is truncated; never modifies files. |
| `fews-xsd-validate` | Validates staged XML files against their XSD schemas (see below). |

## Configuring rules

Put a `.fews-precommit.toml` in the root of your repository (see
[example/.fews-precommit.toml](example/.fews-precommit.toml) for a fully
annotated version):

```toml
# element text must always be this value
[[rules]]
path = "SystemConfigFiles/Explorer.xml"
tag = "adjustSystemTimeAutomatically"
expected = "true"

# only revert known debug values, leave anything else alone
[[rules]]
path = "SystemConfigFiles/Log4jConfig.xml"
tag = "logLevel"
expected = "INFO"
forbidden = ["DEBUG", "TRACE"]

# enforce an attribute instead of the element text
[[rules]]
path = "RegionConfigFiles/*.xml"
tag = "export"
attribute = "debug"
expected = "false"

# disambiguate nested tags with a tag path; compare case-insensitively
[[rules]]
path = "WorkflowFiles/*.xml"
tag = "general/runInLoop"
expected = "false"
ignore_case = true

# a debugging-only element that must never be committed at all
[[rules]]
path = "**/*.xml"
tag = "debugSettings"
remove = true

# an element that must always be present (reported, not auto-fixed)
[[rules]]
path = "SystemConfigFiles/Explorer.xml"
tag = "systemCaption"
expected = "Operational System"
required = true
```

| Key | Meaning |
| --- | --- |
| `path` | Glob matched against the staged file path. Relative patterns also match deeper in the tree, so `SystemConfigFiles/Explorer.xml` matches `Config/SystemConfigFiles/Explorer.xml`. Wildcards like `**/Explorer.xml` work too. |
| `tag` | Name of the XML element to enforce (namespace prefixes are ignored). May be a path like `general/timeOut`, matched against the end of the element nesting, to disambiguate tags that occur in several places. |
| `expected` | The value the element (or attribute) must always have. |
| `forbidden` | Optional. When given, only these values are reverted and any other value is left alone. When omitted, every value other than `expected` is reverted; an empty element or missing attribute is then also filled in. |
| `attribute` | Optional. Enforce this attribute of the element instead of its text content. Note: fixing an attribute normalizes the whitespace inside that one tag. |
| `ignore_case` | Optional, default `false`. Compare values case-insensitively, so `true` and `True` count as equal. |
| `required` | Optional, default `false`. The element must be present in the file. A missing element is reported and fails the hook, but cannot be fixed automatically. |
| `remove` | Optional, default `false`. The element must not be present at all; on fix it is deleted together with its content. Cannot be combined with the other checks. |

Without a `.fews-precommit.toml` the hook falls back to one built-in rule:
`adjustSystemTimeAutomatically` in `SystemConfigFiles/Explorer.xml` must be
`true`.

To keep the rules file somewhere else, pass it explicitly:

```yaml
      - id: fews-xml-defaults
        args: [--config, tools/fews-rules.toml]
```

Only violating values are rewritten — comments, attributes, whitespace, line
endings, BOM and the XML declaration are preserved exactly.

## Validating against the FEWS XSD schemas

Rule checks catch known mistakes; schema validation catches structural ones
(misspelled tags, missing mandatory elements, wrong ordering). This
repository ships a second hook for that, `fews-xsd-validate`, built on
[lxml](https://lxml.de)/libxml2 (validates a full real-world FEWS
configuration in a couple of seconds) with a tolerant pure-Python fallback
for schemas strict libxml2 rejects — pre-commit installs everything
automatically, just like the Rust hook:

```yaml
repos:
  - repo: https://github.com/HKV-products-services/fews-precommit
    rev: v0.1.4
    hooks:
      - id: fews-xml-defaults
      - id: fews-xsd-validate
        args: [--schema-dir, Schemas]
```

How it works:

1. Copy the XSD schemas from your FEWS distribution (or
   `https://fews.wldelft.nl/schemas/`) into a `Schemas/` folder in your
   repository, matching your FEWS version.
2. On commit, the hook reads each staged XML file's `xsi:schemaLocation`,
   looks up the schema **by file name** in the schema directory (so
   `…/version1.0/explorer.xsd` resolves to `Schemas/explorer.xsd`), and
   validates. One hook covers every FEWS file type at once. Relative hints
   (e.g. `../xsd/rtcDataConfig.xsd`, common in RTC-Tools configs) are
   resolved against the XML file's own location.
3. Files that declare no schema are skipped. Files whose schema cannot be
   found are reported but do not fail the hook — tighten with
   `--missing-schema fail`, or allow on-the-fly download of http(s)
   schemaLocation URLs with `--allow-remote` (requires internet on commit;
   does not help for relative hints, whose schema must exist on disk).

Why a Python sidecar instead of validating in the Rust binary: there is no
production-ready pure-Rust XSD validator — see
[docs/xsd-validation.md](docs/xsd-validation.md) for the exploration and
trade-offs.

## Letting Delft-FEWS itself validate the configuration

XSD validation checks each file in isolation; only Delft-FEWS knows whether
the files fit together (location ids referenced by filters, grids,
descriptors, …). The `fews-config-validate` hook runs your locally
installed Delft-FEWS — via its Data Conversion module — against the
repository's configuration in a throwaway region, and fails on every
`ERROR`/`FATAL` line Delft-FEWS logs (Delft-FEWS itself exits 0 even for a
broken configuration, so the log is what counts):

```yaml
      - id: fews-config-validate
        # The Delft-FEWS binaries are machine-specific, so each developer
        # sets the FEWS_EXE environment variable (recommended), e.g.
        #   FEWS_EXE=C:\Data\FEWS\202502\windows\Delft-FEWSc.exe
        # Alternatively pin a path shared by the whole team:
        # args: [--fews-exe, 'C:\Data\FEWS\202502\windows\Delft-FEWSc.exe']
```

Details:

- Requires a local Delft-FEWS installation whose version matches the
  configuration. The region directory (the folder containing `Config`,
  e.g. `FEWS/`) is auto-detected, as is a `*patch*.jar` in that folder;
  override with `--region-dir` and `--patch-jar`.
- A run takes roughly half a minute, so the hook is wired to the
  **pre-push** stage and manual runs instead of every commit: enable with
  `pre-commit install --hook-type pre-push`, or run on demand with
  `pre-commit run fews-config-validate --hook-stage manual`.
- `--test-run <file>` (repeatable) runs a Delft-FEWS **workflow test run**
  instead: a `workflowTestRun` XML (typically in
  `Config/WorkflowTestRuns/`) that executes workflows against the candidate
  configuration — the deepest check available, validating everything those
  workflows touch. See the
  [Deltares wiki](https://publicwiki.deltares.nl/display/FEWSDOC/How+to+configure+a+workflow+test+run.++Running+workflows+from+the+command-line)
  for the file format:

  ```yaml
        - id: fews-config-validate
          args: [--test-run, Config/WorkflowTestRuns/wftr_ImportPvss.xml]
  ```

- `--test-run-all` generates and runs a workflow test run containing
  **every workflow** from `WorkflowDescriptors.xml` — the deepest check
  available from the command line, resolving the module configs, time
  series sets and (time-dependent) related locations of everything it
  runs. It is heavy and workflows needing external systems will fail, so
  scope it (`--include`/`--exclude` regexes on workflow id) and run it in
  CI or nightly rather than on every push:

  ```sh
  fews-config-validate --test-run-all --exclude '^wf_Export' --include '^wf_Import'
  ```

- `--workflow <Id>` (repeatable) additionally runs workflows in
  configuration-load mode, which makes Delft-FEWS load and validate the
  module configs they reference.
- `--global-properties <file>` supplies your own properties file when the
  configuration relies on `$PROPERTY$` expansion (default is a minimal
  built-in one). Workflows that read site properties (connection strings,
  passwords) fail without it.
- `--region-copy <dir>` (repeatable) copies extra region folders (e.g.
  `Modules`) into the throwaway region when workflows run module adapters.
- `--system-time <YYYY-MM-DDTHH:MM:SS>` pins the system time of generated
  test runs (default: now, UTC).
- `--max-memory <MB>` sets the Java heap (default 1024).

### How this maps to the three FEWS validation levels

1. **Errors in the log panel at startup** — covered by the default
   configuration-load mode.
2. **Extra errors when opening the Workflow Navigator (F12 > K)** — the
   navigator is GUI-only; Delft-FEWS 2025.02 has no command-line
   equivalent (also not in `mctools`). A dedicated `configchecktool` would
   be a feature request to Deltares. `--test-run-all` covers the same
   ground from the other side: what the navigator resolves statically, a
   test run of every workflow resolves by execution.
   *Note:* the 2026.01 release notes announce FEWS-34372 "Command line
   configuration checking" (new `checkConfiguration`/`checkGlobalReferences`
   code exists in the Explorer), but in build 605120850 it is undocumented
   and the invocation is not discoverable — ask Deltares for the syntax;
   it can then slot straight into this hook. The hook itself already runs
   fine on 2026.01 binaries: pass the matching patch jar explicitly
   (`--patch-jar <fews-stable-202601-...-patch.jar>`) when your region's
   own patch.jar belongs to an older FEWS version.
3. **A workflow runs successfully** — covered by `--test-run` (your own
   `workflowTestRun` files) or `--test-run-all`.

Example failure output, caught on a real configuration:

```text
Delft-FEWS reported 6 configuration error(s):
  ... ERROR - main.LocationUtils.parseLocationFile - Config.Error: Xml file RegionConfigFiles/Locations.xml is not valid
  ... ERROR - main.TimeSeriesFilter.createFromCastor - Config.Error: Location with id DelfluentHoutrust does not exist
  ... ERROR - main.Grids.initGridGeometries - Config.Error: grid/location id HHD-HARMONIE unknown
```

## Running it by hand

The hook is a normal command-line tool:

```sh
fews-precommit SystemConfigFiles/Explorer.xml          # fix files in place
fews-precommit --check SystemConfigFiles/Explorer.xml  # report only
fews-precommit --check --well-formed *.xml             # also catch syntax errors
fews-precommit --config rules.toml file1.xml file2.xml
```

Exit codes: `0` everything compliant, `1` violations found (and fixed unless
`--check`), `2` configuration, syntax or I/O error.

## Troubleshooting

**"linker `link.exe` not found" during `pre-commit run` (Windows).**
pre-commit compiles the hook with your Rust toolchain, and rustup defaults
to the MSVC toolchain, which needs Visual Studio Build Tools. Either install
those, or switch to the self-contained GNU toolchain:

```sh
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

then run pre-commit again (the failed install is retried automatically).

**The first commit takes minutes.** That is the one-time compilation of the
hook; pre-commit caches the result and later runs take milliseconds.

## Design choices

See [CHANGELOG.md](CHANGELOG.md) for what was implemented when. The main
choices and their rationale:

- **Rust binary behind pre-commit's `language: rust`** — users never touch
  Rust: pre-commit compiles the hook on first use (and can even download the
  toolchain via `language_version`). The result is a fast, dependency-free
  native binary on every platform FEWS teams use.
- **Fix by default, fail the commit** — like rustfmt/black, the hook repairs
  the file and exits non-zero so the user consciously re-stages. A check-only
  variant exists for CI or cautious teams.
- **Event-streaming rewrite instead of DOM or regex** — quick-xml events are
  copied through verbatim and only the violating value is replaced. This
  preserves formatting byte-for-byte (comments, CRLF, BOM, attribute order),
  which keeps diffs minimal — important for config repositories under review.
  Regex would be formatting-safe but blind to XML structure (CDATA, comments,
  nesting); a DOM round-trip would reformat the whole file.
- **TOML rules file, not args** — FEWS teams maintain many rules; a
  `.fews-precommit.toml` in the repo root keeps them versioned, reviewable
  and out of `.pre-commit-config.yaml`. Built-in defaults make the hook work
  with zero configuration.
- **Conservative matching** — exact value comparison by default (opt-in
  `ignore_case`), and the `forbidden` list limits reverts to known debug
  values so legitimate custom values survive.
- **No full XSD validation in the binary** — the Rust ecosystem has no mature
  XSD 1.0 validator and binding libxml2 would break the "compiles anywhere
  without prerequisites" property. Schema validation is delegated to a
  Python sidecar hook (see above).

## Development

```sh
cargo test    # run the test suite (unit + end-to-end CLI tests)
cargo build --release
```

CI runs the tests on Linux, Windows and macOS, lints with rustfmt/clippy, and
exercises the hook through `pre-commit try-repo` exactly as end users consume
it.
