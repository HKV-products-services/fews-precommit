# fews-precommit

A [pre-commit](https://pre-commit.com) hook for Delft-FEWS configuration
repositories. It scans staged XML files and reverts values that were changed
for debugging or testing back to their required defaults, so they never end
up in a commit.

The classic example: while debugging it is convenient to set

```xml
<adjustSystemTimeAutomatically>False</adjustSystemTimeAutomatically>
```

in `SystemConfigFiles/Explorer.xml`, but in the repository this must always be

```xml
<adjustSystemTimeAutomatically>True</adjustSystemTimeAutomatically>
```

This hook reverts it automatically on commit.

## Usage

You only need `git`, `python` and [pre-commit](https://pre-commit.com)
installed — no Rust knowledge required. Add this to the
`.pre-commit-config.yaml` of your FEWS configuration repository:

```yaml
repos:
  - repo: https://github.com/HKV-products-services/fews-precommit
    rev: v0.1.0
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

SystemConfigFiles/Explorer.xml: <adjustSystemTimeAutomatically> is 'False', must be 'True' -- reverted
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
expected = "True"

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
`True`.

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
repository ships a second hook for that, `fews-xsd-validate`, built on the
Python [xmlschema](https://github.com/sissaschool/xmlschema) package —
pre-commit installs it automatically, just like the Rust hook:

```yaml
repos:
  - repo: https://github.com/HKV-products-services/fews-precommit
    rev: v0.1.0
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
   validates. One hook covers every FEWS file type at once.
3. Files that declare no schema are skipped. Files whose schema is not in
   the folder are reported but do not fail the hook — tighten with
   `--missing-schema fail`, or allow on-the-fly download from the
   schemaLocation URL with `--allow-remote` (requires internet on commit).

Why a Python sidecar instead of validating in the Rust binary: there is no
production-ready pure-Rust XSD validator — see
[docs/xsd-validation.md](docs/xsd-validation.md) for the exploration and
trade-offs.

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
