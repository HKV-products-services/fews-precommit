# Review: fews-precommit

*Covers v0.1.0 (2026-06-10) through the unreleased work of 2026-06-12.
All verification was performed on Windows 11 with Rust 1.96.0 (GNU
toolchain), Python 3.12.10, Delft-FEWS 2025.02 (build 605200839) and
2026.01 (build 605120850), against the real `FEWS_HHD/BOS` configuration
repository (325 config files, full schema set).*

## Verdict

The tool suite is released (v0.1.4 on
`github.com/HKV-products-services/fews-precommit`), consumed through
pre-commit exactly as colleagues would use it, and field-tested on a
production FEWS configuration where it has already caught real errors at
every layer it checks. Four hooks cover value rules, XML well-formedness,
XSD schema validation, and validation by Delft-FEWS itself — the deepest
check available from the command line today. The one structural gap
("level 2", the Workflow Navigator resolve) is a Delft-FEWS limitation,
not a tooling one: the 2026.01 feature that should fill it (FEWS-34372)
exists in the binaries but is undocumented and not yet invocable.

## The hooks and what they catch

| Hook | Checks | Speed | Stage |
| --- | --- | --- | --- |
| `fews-xml-defaults` | Debug values reverted to required defaults (element text, attributes, forbidden elements, required elements) | ms | every commit |
| `fews-xml-wellformed` | XML syntax errors, truncated files | ms | every commit |
| `fews-xsd-validate` | Schema validation per file (structure, ordering, enums) | ~2 s for the whole repo | every commit |
| `fews-config-validate` | Delft-FEWS loads the config: broken cross-file references, unknown ids; optionally runs workflows | ~30 s per run | pre-push / manual / CI |

## What was verified

| # | Check | Result |
| --- | --- | --- |
| 1 | Rust quality gates (`fmt`, `clippy -D warnings`) and 37 automated tests (22 unit, 15 end-to-end CLI) | Pass |
| 2 | Real pre-commit consumption (`try-repo`): fix hook reverts and fails, re-run passes, XSD and well-formedness hooks pass/fail correctly | Pass |
| 3 | Field test on BOS: `pre-commit run --all-files` | Works; two findings, both fixed (see below) |
| 4 | XSD engine benchmark on BOS (325 files) | lxml 1.9 s vs pure-Python 26.7 s (~14×), identical verdicts |
| 5 | XSD fallback for a broken FEWS-shipped schema | Works (see findings) |
| 6 | `fews-config-validate` configuration-load mode on clean BOS | Exit 0, ~30 s, repository untouched afterwards |
| 7 | Same, on a deliberately broken config (truncated `Locations.xml`) | Exit 1 with all 6 cascade errors |
| 8 | Workflow test run mode (Job Verkaik's WorkflowTestRun approach, incl. JVM flags; heap verified at 1024 MB) | Works; full multi-line FEWS errors captured |
| 9 | `--test-run-all` scoped to `wf_ImportPvss` on BOS | Works; found a real config error on first use |
| 10 | `fews-config-validate` on Delft-FEWS **2026.01** binaries with the user-supplied 2026.01 patch jar | Exit 0 |

## Findings in the BOS configuration (real errors caught)

1. `ConstructWaterbalance.xml`: `<valueType>` in a `timeSeriesSet` violates
   the schema's element order (XSD hook).
2. `LocationSets.xml`: nested LocationSet `locaties.test` referenced by
   `locaties.pvss.test` does not exist (`--test-run-all`) — exactly the
   "related locations" error class the team says causes most SPOC
   deadlocks, invisible to XSD.
3. Both `rtcDataConfig.xml`'s schema (`..\bin\rtcDataConfig.xsd`, not in
   the repo) and site property keys (`PVSS-CONNECTION-STRING`, …) are
   environment-dependent: supply the XSD in `Schemas/` and pass
   `--global-properties` for workflows that read site properties.

## Findings about Delft-FEWS itself

- **Exit codes lie.** The Data Conversion module exits 0 even when the
  configuration is broken; the hook therefore parses the run log for
  `ERROR`/`FATAL` lines (multi-line entries captured whole).
- **A FEWS-shipped schema is malformed**: `taskProperties.xsd` contains
  `schemaLocation="mcSynchronisation.xsd "` (trailing space). Strict
  libxml2 refuses it; the XSD hook falls back to the tolerant pure-Python
  engine for such schemas so no file goes unvalidated.
- **The three validation levels** (startup log; Workflow Navigator
  F12 > K; a workflow runs) map to: configuration-load mode covers
  level 1; `--test-run`/`--test-run-all` cover level 3 and, by executing
  workflows, most of what level 2 resolves statically. A direct level-2
  command does not exist in 2025.02: the Workflow Navigator is GUI-only,
  and `mctools` has no config check tool.
- **FEWS-34372 ("Command line configuration checking", 2026.01)**: the
  implementation exists — `checkConfiguration` and
  `checkGlobalReferences` are new in the 2026.01 Explorer — but the
  release-notes documentation column says "None" and the invocation is
  not discoverable from the binaries: the Explorer's command-line
  vocabulary in build 605120850 is only
  `requestFocus`/`runIfdTask`/`runIfdTaskAutoSaveToServer`/`runTask`, and
  probes with `validate`/`checkconfig`/`configcheck` are rejected.
  **Action: ask Deltares for the syntax**; it can then be added to
  `fews-config-validate` as the proper level-2 mode in a few lines.
- **2026.01 launch model changed**: a `patch.jar` is required in the
  region home and the Explorer expects to be started via the new
  launcher. The hook's main classes (DCM, WorkflowTestRun) are unaffected
  — verified working — but cross-version runs must pass the matching
  patch jar explicitly (`--patch-jar`); patch jars remain user-supplied.

## Strengths

- **Zero-knowledge install, proven** — pre-commit builds the Rust binary
  and Python environments itself; verified via `try-repo`, not assumed.
- **Layered defence at the right costs** — millisecond rule/syntax checks
  on every commit, second-scale XSD on every commit, half-minute
  FEWS-validation on push/CI; each layer caught real errors during
  development or field testing.
- **Formatting-preserving fixes** — one-line diffs; comments, CRLF, BOM,
  attribute order survive byte-for-byte.
- **Safety around the real repository** — `fews-config-validate` works in
  a throwaway region whose Config is a link; cleanup removes the link
  first so it can never touch repo files (verified: BOS unchanged after
  every run).
- **Honest reporting** — adoption-friendly defaults (`--missing-schema
  warn`), messages that say what to do, documented limitations instead of
  silent gaps.

## Weaknesses and limitations

- **Level 2 remains a gap** until Deltares documents FEWS-34372;
  `--test-run-all` approximates it by execution, which is heavier and can
  produce environment-dependent failures (site properties, missing module
  binaries) that need `--global-properties`/`--region-copy`/`--exclude`
  scoping.
- **`fews-config-validate` cannot run in GitHub-hosted CI** — it needs
  the Delft-FEWS binaries. CI covers the Rust and XSD hooks; the FEWS
  validation layer needs a self-hosted runner (or the
  `deltares/delft-fews` Docker image idea from the SPOC discussion).
- **Per-machine setup** — `FEWS_EXE` must be set by each developer, and
  FEWS binaries/patch jars must match the configuration version.
- **The Python modules have no pytest suite** — they are covered by CI
  shell checks and manual field tests only; the Rust core has the real
  test suite.
- **Schema resolution is by file name** — version mismatches between a
  pinned `Schemas/` folder and the FEWS build are silent.
- **The example XSD is a stub**, clearly marked as such.
- **Uncommitted-work fragility** — once during development, a `git
  checkout` in the working tree silently reverted in-progress files
  (recovered). Releases are tagged immediately since.

## Room for improvement

### Next

1. Release the unreleased work as v0.1.5 (`--test-run-all`,
   `--region-copy`, `--system-time`, FEWS-34372/2026.01 notes).
2. Ask Deltares (or watch the wiki) for the FEWS-34372 invocation and add
   it as the level-2 mode.
3. Fix the two real BOS findings (`locaties.test` LocationSet,
   `ConstructWaterbalance.xml` element order) and add the missing
   `rtcDataConfig.xsd` to `Schemas/`.

### Soon

1. Wire `fews-config-validate` into team CI on a self-hosted runner with
   a CI-safe properties file; nightly `--test-run-all` with curated
   `--exclude` list.
2. Add a pytest suite plus `ruff` for the two Python modules.
3. Add `.gitattributes` to settle line endings.

### Later

1. Prebuilt binaries / release automation; dependabot + `cargo audit`.
2. Schema-version awareness in the XSD hook.
3. Re-evaluate pure-Rust XSD validation yearly (`xmloxide`).
4. The `deltares/delft-fews` Docker image for `uses:`-style GitHub
   Actions integration — Deltares-side, aligned with the SPOC plan.
