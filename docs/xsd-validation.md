# Exploring XSD schema validation for FEWS configurations

Delft-FEWS ships an XSD schema for every configuration file type (Explorer,
Workflows, ModuleConfigFiles, …), published at
`https://fews.wldelft.nl/schemas/` and included in the FEWS installation
directory. Validating staged XML against those schemas would catch a whole
class of mistakes that value rules cannot: misspelled tags, missing mandatory
elements, wrong element order, invalid enum values.

This document records the options that were evaluated and why the current
recommendation is a Python sidecar hook rather than building validation into
the `fews-precommit` binary.

## Options evaluated

### 1. Pure-Rust XSD validation — not viable today

The Rust ecosystem has no production-ready XSD 1.0 validator:

- The [`xmlschema` crate](https://crates.io/crates/xmlschema) is abandoned at
  v0.0.1 (last update February 2023, ~2600 downloads total).
- [`fastxml`](https://github.com/reearth/fastxml) parses XSD schemas but is
  v0.x, early development, and explicitly "not recommended for
  business-critical systems".
- [`xsd-parser`](https://crates.io/crates/xsd-parser) generates Rust types
  *from* a schema at build time — useful for code generation, not for
  validating arbitrary documents against arbitrary schemas at runtime.
- [`xmloxide`](https://github.com/jonwiggins/xmloxide), a pure-Rust libxml2
  reimplementation with schema support, is promising but young.

FEWS schemas are large and use most of XSD (imports, substitution groups,
identity constraints), so a partial implementation would give false
confidence. Worth re-evaluating yearly; `xmloxide` is the one to watch.

### 2. libxml2 bindings (`rust-libxml`) — works, but breaks the install story

[rust-libxml](https://github.com/KWARC/rust-libxml) exposes libxml2's mature
schema validator. The blocker: every user's machine would need the native
libxml2 library at compile time. pre-commit builds this hook from source on
the user's machine — on Windows (the typical FEWS workstation) that means
manually installing libxml2 first, which defeats the goal that the hook works
"without any knowledge of Rust" or anything else. Could become an optional
cargo feature later, off by default.

### 3. Shelling out to `xmllint` — too fragile

`xmllint --schema explorer.xsd Explorer.xml` works well where libxml2 tools
are installed, but they aren't on Windows by default, so the hook would work
for some team members and silently degrade for others.

### 4. Python `xmlschema` package — recommended

[xmlschema](https://github.com/sissaschool/xmlschema) is a mature, pure-Python
XSD 1.0/1.1 validator. Two properties make it the right fit:

- pre-commit installs Python hook environments (including
  `additional_dependencies`) automatically — same zero-knowledge install as
  the Rust hook.
- It ships an `xmlschema-validate` CLI: positional XML files,
  `--schema PATH/URL`, `--version 1.0|1.1`, `-L` for schema-location
  overrides; the exit code is the number of validation errors, which is
  exactly what pre-commit needs.

## The implemented setup: the `fews-xsd-validate` hook

This repository ships the recommended setup as a ready-made hook
(implemented in [fews_xsd_validate.py](../fews_xsd_validate.py), packaged
via [pyproject.toml](../pyproject.toml)):

```yaml
repos:
  - repo: https://github.com/HKV-products-services/fews-precommit
    rev: v0.1.0
    hooks:
      - id: fews-xsd-validate
        args: [--schema-dir, Schemas]
```

Rather than calling `xmlschema-validate --schema X` directly — which forces
a single schema for every file pre-commit passes, and therefore one hook
per FEWS file type — the wrapper resolves the schema **per file**: it reads
each file's `xsi:schemaLocation` (or `xsi:noNamespaceSchemaLocation`) and
looks the schema up by file name in the local schema directory, so
`…/version1.0/explorer.xsd` resolves to `Schemas/explorer.xsd`. One hook
covers the whole configuration; parsed schemas are cached across files
within a run.

Behavior:

- Files that declare no schema are skipped.
- Files whose schema is not in the schema directory are reported but pass
  by default (`--missing-schema warn`), so teams can adopt validation
  incrementally, schema by schema. Use `--missing-schema fail` once the
  directory is complete, or `--missing-schema skip` for silence.
- `--allow-remote` falls back to downloading the XSD from the declared URL.
  Off by default: it needs internet access on every commit and validates
  against whatever is published rather than a pinned version.
- Exit codes: `0` all validated files pass, `1` validation errors, `2`
  operational errors (unreadable file, broken schema, missing schema with
  `--missing-schema fail`).

Notes:

- **Keep a copy of the XSDs in the repository** (e.g. a `Schemas/` folder
  copied from the FEWS installation's `Config/.../schemas` or downloaded from
  `https://fews.wldelft.nl/schemas/version1.0/`), matched to your FEWS
  build — schemas evolve between FEWS releases. Pinned local schemas
  guarantee everyone validates against the same version, offline.
- XSD validation is slower than the Rust rule checks (seconds rather than
  milliseconds for large configs), which is why it is a separate, opt-in
  hook rather than part of `fews-xml-defaults`.

## What the Rust hook already covers

Without XSD, `fews-precommit` still rejects structurally broken files cheaply:
the `fews-xml-wellformed` hook parses every staged XML file and fails on
syntax errors, mismatched end tags and truncated files. Combined with value
rules this covers the "accidentally committed debug state" and "file is
corrupt" classes; XSD adds the "structurally invalid configuration" class.

## Sources

- [xmlschema (Python) on GitHub](https://github.com/sissaschool/xmlschema) —
  CLI entry points `xmlschema-validate`, `xmlschema-xml2json`,
  `xmlschema-json2xml`
- [xmlschema documentation](https://xmlschema.readthedocs.io/)
- [xmlschema crate (Rust)](https://crates.io/crates/xmlschema) — abandoned
- [fastxml (Rust)](https://github.com/reearth/fastxml)
- [xsd-parser (Rust)](https://crates.io/crates/xsd-parser/1.1.0)
- [xmloxide (Rust)](https://github.com/jonwiggins/xmloxide)
- [rust-libxml schema example](https://github.com/KWARC/rust-libxml/blob/master/examples/schema_example.rs)
- [Rust forum: XML XSD validation](https://users.rust-lang.org/t/xml-xsd-validation-and-xsl-transform/30393)
