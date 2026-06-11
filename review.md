# Review: fews-precommit

*Verification performed 2026-06-10 on Windows 11, Rust 1.96.0 (GNU toolchain),
Python 3.12.10, in clean temporary environments.*

## Verdict

The tool works end-to-end, exactly the way a colleague would consume it: all
three pre-commit hooks were exercised through `pre-commit try-repo` against a
simulated FEWS configuration repository, and behaved as designed. The code is
tested (37 automated tests), lint-clean, and documented. The repository has
since been published at `github.com/HKV-products-services/fews-precommit`
and tagged `v0.1.0`; the post-review field-test fixes below ship as
`v0.1.1`.

## What was verified

| # | Check | Method | Result |
| --- | --- | --- | --- |
| 1 | Code quality gates | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` | Pass, zero warnings |
| 2 | Automated tests | `cargo test` | 37/37 pass (22 unit, 15 end-to-end CLI) |
| 3 | Release build | `cargo build --release` | Builds clean |
| 4 | Python package installs | `pip install .` in a fresh venv | Installs, `fews-xsd-validate` CLI works |
| 5 | XSD: valid file | example Explorer.xml vs example schema | Exit 0 |
| 6 | XSD: invalid file | injected `<bogusElement/>` | Exit 1, precise message (element path + expected tag) |
| 7 | XSD: missing / undeclared schema | unknown.xsd hint; schema-less XML | Warn+pass by default, exit 2 with `--missing-schema fail`, skip when no declaration |
| 8 | **Real consumption: fix hook** | `pre-commit try-repo` from a committed snapshot; consumer repo with staged `False` | pre-commit compiled the hook with cargo; run **Failed**, file reverted to `True`, message shown |
| 9 | **Real consumption: re-run after fix** | same, after re-staging | **Passed** (exit 0) |
| 10 | **Real consumption: XSD hook** | `try-repo fews-xsd-validate` | Passed on valid file, Failed with validation error on broken file |
| 11 | **Real consumption: well-formedness** | `try-repo fews-xml-wellformed` on `<explorer><unclosed>` | Failed: "XML is truncated: 2 unclosed element(s)" |

A side finding during verification: `pip install` had left `build/` and
`*.egg-info/` artifacts in the project root that were not gitignored — fixed
during this review (`.gitignore` now covers Python artifacts).

**Post-review field test (same day):** the hook was run on a real FEWS
configuration repository (`FEWS_HHD/BOS`) via `pre-commit run --all-files`.
Two issues surfaced and were fixed:

1. On machines without Visual Studio Build Tools, pre-commit's
   `cargo install` fails with "linker `link.exe` not found" because rustup
   defaults to the MSVC toolchain. Fix on such machines:
   `rustup default stable-x86_64-pc-windows-gnu` (documented in the README
   troubleshooting section).
2. The original plan's example value `True` (capitalized) is wrong for real
   FEWS configurations: the schemas use `xs:boolean`, which only accepts
   lowercase `true`/`false`. The built-in default, examples and docs now
   enforce lowercase. After both fixes, the hook ran green on the real
   repository and correctly enforces the schema-valid value.

## Strengths

- **Zero-knowledge install.** Users add three lines of YAML; pre-commit
  builds the Rust binary and the Python validator environment itself. This
  was proven with `try-repo`, not assumed.
- **Formatting-preserving fixes.** Only the violating value changes;
  comments, attribute order, CRLF line endings, UTF-8 BOM, and the XML
  declaration survive byte-for-byte. Diffs stay one line — important for
  reviewed configuration repositories.
- **Expressive but conservative rule model.** Element text, attributes, tag
  paths for disambiguation, forbid-element (`remove`), require-element,
  case-insensitive comparison, and `forbidden`-list scoping so only known
  debug values are reverted. Invalid rule combinations are rejected at
  config load with clear messages.
- **Defensive about broken input.** Truncated XML, mismatched tags, CDATA,
  tail text after child elements, and non-UTF-8 files are all handled —
  several of these were real bugs found by the test suite during
  development, which is what the tests are for.
- **Layered checking.** Cheap Rust rule checks on every commit; optional
  XSD validation (structural correctness) as a separate hook with
  per-file schema resolution, so one hook line covers every FEWS file type.
- **Honest, complete documentation.** README with copy-paste configs,
  annotated example, changelog with design rationale, and a written
  exploration of the XSD options including why pure-Rust validation was
  rejected.
- **CI worth trusting.** Tests on Linux/Windows/macOS, lint gates, and an
  integration job that consumes the hooks via `pre-commit try-repo` — the
  same path verified manually here.

## Weaknesses and limitations

- **CI not yet confirmed green on GitHub's runners.** The repository is
  published and tagged, but the workflow results on real runners should be
  checked (everything was replicated locally, which is not the same as a
  green badge).
- **First-use build cost.** pre-commit compiles the Rust hook once per
  machine (minutes). Cached afterwards, but the first commit surprises
  people. Pinning `language_version` mitigates the no-toolchain case but was
  not tested against pre-commit's rustup download path.
- **The example XSD is a stub.** A minimal demonstration schema, not the
  real `explorer.xsd`. The validator has not been tested against the
  full-size FEWS schemas (large, with imports) or against a real
  multi-thousand-file FEWS configuration; performance there is unmeasured.
- **Schema resolution is by file name only.** `Schemas/explorer.xsd` wins
  regardless of which FEWS version the file's URL pins. Acceptable when the
  team keeps one schema set per repo, but version mismatches are silent.
- **Rule-engine edges.** Namespace prefixes are ignored when matching tags
  (fine for FEWS, wrong for exotic XML); fixing an attribute normalizes
  whitespace inside that one tag; `ignore_case` is ASCII-only; `required`
  applies to *every* file the glob matches, which can surprise with broad
  globs like `**/*.xml`; when several rules match the same element, the
  first in the file wins, undocumented.
- **Windows path cosmetics.** Messages may mix `\` and `/` in paths
  (e.g. `SystemConfigFiles\Explorer.xml`), harmless but slightly untidy.
- **No `.gitattributes`.** Git warned about LF/CRLF normalization on many
  files during verification; line endings are currently left to each
  contributor's git config.
- **Local Windows development quirk** (developers of this tool only, not
  users): the default MSVC toolchain needs Visual Studio Build Tools; on
  this machine the GNU toolchain is used instead. Not documented in the
  README's development section.

## Room for improvement

Prioritized; the first block is what stands between "works on my machine"
and "team-ready".

### Before sharing with the team

1. Commit and tag the `v0.1.1` fixes (boolean casing, troubleshooting docs)
   and confirm CI is green on GitHub's runners.
2. Drop the real FEWS XSDs (matching the team's FEWS version) into a pilot
   repository's `Schemas/` folder and run `fews-xsd-validate` against the
   actual configuration once, to size up performance and schema coverage.
3. Pilot on one real FEWS configuration repository with 2–3 rules before
   rolling out broadly.

### Soon after

1. Add a `.gitattributes` (`* text=auto`, `*.rs text eol=lf`, …) to settle
   line endings.
2. Test and document the no-toolchain path (`language_version` pinning) on
   a machine without Rust.
3. Add a pytest suite for `fews_xsd_validate.py` (currently covered only by
   CI shell checks) and run `ruff`/`mypy` over it in CI.
4. Document rule precedence (first match wins) and the per-file semantics
   of `required` in the README.

### Later / nice to have

1. A `--list-rules` or `--dry-run` flag showing which rules apply to which
   staged files, for debugging configurations.
2. Schema-version awareness in the XSD hook (e.g. warn when the local XSD
   differs from the version pinned in the file's URL).
3. Release automation: tagged GitHub releases with prebuilt binaries
   and/or publishing to crates.io, plus dependabot/`cargo audit` for
   dependency hygiene.
4. Re-evaluate pure-Rust XSD validation yearly (watch `xmloxide`); if it
   matures, the Python sidecar could fold into the single binary.
5. Richer rule types if demand appears: regex-constrained values, numeric
   ranges, enforcing element presence with auto-insert templates.
