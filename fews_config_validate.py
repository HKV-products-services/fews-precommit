"""Validate a Delft-FEWS configuration by letting Delft-FEWS itself load it.

Two modes, both running through the user's Delft-FEWSc executable against a
throwaway region whose Config points at the repository's configuration:

1. Default: boot the Data Conversion module (part of every Delft-FEWS
   distribution). Delft-FEWS parses the region configuration and reports
   semantic errors XSD validation cannot see: broken references between
   files, unknown location/grid ids, and so on.
2. --test-run <workflowTestRun.xml>: run a Delft-FEWS workflow test run,
   which additionally executes workflows against the candidate
   configuration (see the Deltares wiki: "How to configure a workflow test
   run. Running workflows from the command-line").

The Delft-FEWS binaries are machine specific and therefore user defined:
pass --fews-exe or set the FEWS_EXE environment variable to e.g.
C:\\Data\\FEWS\\202502\\windows\\Delft-FEWSc.exe.

Note: the Data Conversion module exits 0 even when the configuration has
errors, so this wrapper also parses the run log for ERROR/FATAL lines.

Part of fews-precommit; installed by pre-commit as the fews-config-validate
hook.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

DCM_MAIN = "nl.wldelft.dataconversion.DataConversionModule"
TEST_RUN_MAIN = "nl.wldelft.fews.system.workflowtestrun.WorkflowTestRun"

ERROR_LINE = re.compile(r"\b(ERROR|FATAL)\b")
LOG_TIMESTAMP = re.compile(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}")

MINIMAL_GLOBAL_PROPERTIES = "localDatastoreFormat=hsqldb\n"

# Minimal valid Data Conversion config: booting the module is what loads and
# validates the region configuration; the import status activity is a no-op.
DATACONVERSION_XML = """\
<?xml version="1.0" encoding="UTF-8"?>
<dataConversion xmlns="http://www.wldelft.nl/fews">
    <activities>
{workflows}        <importStatusActivity id="ImportStatus">
            <importStatusPath>%REGION_HOME%/ImportStatus</importStatusPath>
        </importStatusActivity>
    </activities>
</dataConversion>
"""

WORKFLOW_ACTIVITY = """\
        <workflowActivity>
            <workflowId>{workflow}</workflowId>
        </workflowActivity>
"""


def find_region_dir(region_arg: Path | None) -> Path | None:
    """The region dir is the one containing Config/. Defaults to '.' or the
    single direct subdirectory that has a Config folder (e.g. 'FEWS')."""
    if region_arg is not None:
        return region_arg if (region_arg / "Config").is_dir() else None
    if Path("Config").is_dir():
        return Path(".")
    candidates = [p for p in Path(".").iterdir() if (p / "Config").is_dir()]
    return candidates[0] if len(candidates) == 1 else None


def find_patch_jar(region_dir: Path) -> Path | None:
    jars = sorted(region_dir.glob("*patch*.jar"))
    return jars[-1] if jars else None


def link_or_copy_config(config_dir: Path, target: Path) -> bool:
    """Make `target` point at config_dir; returns True when it is a link
    (which must be removed with rmdir, never rmtree)."""
    source = str(config_dir.resolve())
    try:
        os.symlink(source, target, target_is_directory=True)
        return True
    except OSError:
        pass
    if os.name == "nt":
        # Directory junctions need no special privileges on Windows.
        result = subprocess.run(
            ["cmd", "/c", "mklink", "/J", str(target), source],
            capture_output=True,
        )
        if result.returncode == 0:
            return True
    shutil.copytree(source, target)
    return False


def resolve_test_run(value: str, region_dir: Path, temp_region: Path) -> Path | None:
    """Map a workflowTestRun file to its path inside the throwaway region.

    Files under Config are reachable through the Config link; anything else
    (e.g. <region>/WorkflowTestRuns/) is copied into the throwaway region.
    """
    given = Path(value)
    candidates = (
        [given]
        if given.is_absolute()
        else [region_dir / given, region_dir / "Config" / given, given]
    )
    for candidate in candidates:
        if not candidate.is_file():
            continue
        resolved = candidate.resolve()
        config_root = (region_dir / "Config").resolve()
        try:
            relative = resolved.relative_to(config_root)
        except ValueError:
            target = temp_region / "WorkflowTestRuns" / resolved.name
            target.parent.mkdir(exist_ok=True)
            shutil.copy(resolved, target)
            return target
        return temp_region / "Config" / relative
    return None


def build_command(
    fews_exe: Path,
    patch_jar: Path | None,
    region: Path,
    max_memory_mb: int,
    main_class: str,
    extra_args: list[str],
) -> list[str]:
    exe = fews_exe.resolve()
    # <root>/windows/Delft-FEWSc.exe: jars in <root>, native dlls next to exe.
    bin_dir = exe.parent.parent
    separator = "\\" if os.name == "nt" else "/"
    command = [
        str(exe),
        "-Djava.locale.providers=SPI,JRE",
        "-Dsun.err.encoding=UTF-8",
        "-Dsun.stdout.encoding=UTF-8",
        f"-Dregion.home={region}",
        "-DautoRollingBarrel=false",
        f"-Xmx{max_memory_mb}m",
        f"-Djava.library.path={exe.parent}",
    ]
    log4j = region / "log4j2.xml"
    if log4j.is_file():
        command.append(f"-Dlog4j.configurationFile={log4j}")
    index = 1
    if patch_jar is not None:
        command.append(f"-Wclasspath.{index}={patch_jar.resolve()}")
        index += 1
    command.append(f"-Wclasspath.{index}={bin_dir}{separator}*.jar")
    command.append(f"-Wmain.class={main_class}")
    command += extra_args
    return command


def collect_problems(output: str, log_file: Path) -> list[str]:
    """ERROR/FATAL log entries from the console output and the region log,
    deduplicated while keeping their order. A log entry may continue on the
    following lines (until the next timestamped line)."""
    lines = output.splitlines()
    if log_file.is_file():
        lines += log_file.read_text(errors="replace").splitlines()
    problems: list[str] = []
    seen: set[str] = set()
    index = 0
    while index < len(lines):
        stripped = lines[index].strip()
        index += 1
        if not ERROR_LINE.search(stripped):
            continue
        entry = [stripped]
        while (
            index < len(lines)
            and not LOG_TIMESTAMP.match(lines[index].strip())
            and len(entry) < 6
        ):
            if lines[index].strip():
                entry.append(lines[index].strip())
            index += 1
        message = " ".join(entry)
        if message not in seen:
            seen.add(message)
            problems.append(message)
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="fews-config-validate",
        description="Validate a Delft-FEWS configuration by letting "
        "Delft-FEWS itself load it (and optionally run workflow test runs).",
    )
    parser.add_argument(
        "--fews-exe",
        type=Path,
        default=os.environ.get("FEWS_EXE"),
        help="path to Delft-FEWSc.exe, e.g. "
        "C:\\Data\\FEWS\\202502\\windows\\Delft-FEWSc.exe. Machine specific; "
        "defaults to the FEWS_EXE environment variable",
    )
    parser.add_argument(
        "--region-dir",
        type=Path,
        default=None,
        help="directory containing the Config folder (default: auto-detect "
        "'.' or a single subdirectory with a Config folder, e.g. 'FEWS')",
    )
    parser.add_argument(
        "--patch-jar",
        type=Path,
        default=None,
        help="FEWS patch jar to put on the classpath (default: newest "
        "*patch*.jar in the region directory, if any)",
    )
    parser.add_argument(
        "--global-properties",
        type=Path,
        default=None,
        help="global.properties to use for the validation region (default: "
        "a minimal built-in one)",
    )
    parser.add_argument(
        "--test-run",
        action="append",
        default=[],
        metavar="FILE",
        help="run this workflowTestRun XML (e.g. "
        "Config/WorkflowTestRuns/wftr_ImportPvss.xml) instead of only "
        "loading the configuration (repeatable)",
    )
    parser.add_argument(
        "--workflow",
        action="append",
        default=[],
        metavar="ID",
        help="in configuration-load mode, additionally run this workflow "
        "via the Data Conversion module (repeatable)",
    )
    parser.add_argument(
        "--max-memory",
        type=int,
        default=1024,
        metavar="MB",
        help="Java heap size for the Delft-FEWS run (default: 1024)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=900,
        help="seconds before a Delft-FEWS run is aborted (default: 900)",
    )
    # pre-commit passes the staged file names; they are irrelevant because
    # Delft-FEWS always validates the configuration as a whole.
    parser.add_argument("files", nargs="*", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)

    if args.fews_exe is None:
        print(
            "fews-config-validate: no Delft-FEWS executable configured; "
            "set the FEWS_EXE environment variable or pass --fews-exe "
            "(e.g. C:\\Data\\FEWS\\202502\\windows\\Delft-FEWSc.exe)"
        )
        return 2
    fews_exe = Path(args.fews_exe)
    if not fews_exe.is_file():
        print(f"fews-config-validate: {fews_exe} does not exist")
        return 2

    region_dir = find_region_dir(args.region_dir)
    if region_dir is None:
        print(
            "fews-config-validate: no Config folder found; run from the "
            "repository root or pass --region-dir"
        )
        return 2
    patch_jar = args.patch_jar or find_patch_jar(region_dir)

    temp_region = Path(tempfile.mkdtemp(prefix="fews-config-validate-"))
    config_link = temp_region / "Config"
    is_link = False
    try:
        is_link = link_or_copy_config(region_dir / "Config", config_link)
        (temp_region / "ImportStatus").mkdir()
        if args.global_properties is not None:
            shutil.copy(args.global_properties, temp_region / "global.properties")
        else:
            (temp_region / "global.properties").write_text(MINIMAL_GLOBAL_PROPERTIES)
        if (region_dir / "log4j2.xml").is_file():
            shutil.copy(region_dir / "log4j2.xml", temp_region / "log4j2.xml")

        runs: list[tuple[str, list[str]]] = []
        if args.test_run:
            for value in args.test_run:
                test_run = resolve_test_run(value, region_dir, temp_region)
                if test_run is None:
                    print(
                        f"fews-config-validate: workflow test run file "
                        f"'{value}' not found"
                    )
                    return 2
                command = build_command(
                    fews_exe,
                    patch_jar,
                    temp_region,
                    args.max_memory,
                    TEST_RUN_MAIN,
                    [f"-Warg.1={temp_region}", f"-Warg.2={test_run}"],
                )
                runs.append((f"workflow test run {value}", command))
        else:
            workflows = "".join(
                WORKFLOW_ACTIVITY.format(workflow=w) for w in args.workflow
            )
            (temp_region / "dataconversion.xml").write_text(
                DATACONVERSION_XML.format(workflows=workflows)
            )
            command = build_command(
                fews_exe,
                patch_jar,
                temp_region,
                args.max_memory,
                DCM_MAIN,
                [f"regionpath={temp_region}", "configfile=dataconversion.xml"],
            )
            runs.append(("configuration load", command))

        failed = False
        log_file = temp_region / "log.txt"
        for label, command in runs:
            try:
                result = subprocess.run(
                    command,
                    capture_output=True,
                    text=True,
                    errors="replace",
                    timeout=args.timeout,
                )
            except subprocess.TimeoutExpired:
                print(
                    f"fews-config-validate: {label} exceeded {args.timeout}s"
                )
                return 2

            output = (result.stdout or "") + (result.stderr or "")
            problems = collect_problems(output, log_file)
            log_file.unlink(missing_ok=True)
            if problems:
                failed = True
                print(
                    f"{label}: Delft-FEWS reported "
                    f"{len(problems)} error(s):"
                )
                for line in problems:
                    print(f"  {line}")
            elif result.returncode != 0:
                failed = True
                print(
                    f"{label}: Delft-FEWS exited with {result.returncode}; "
                    f"last output:"
                )
                for line in output.splitlines()[-15:]:
                    print(f"  {line}")
        return 1 if failed else 0
    finally:
        # The Config entry may be a link into the real repository: remove the
        # link itself first so the cleanup can never touch the repo files.
        if is_link:
            try:
                os.rmdir(config_link)
            except OSError:
                pass
        shutil.rmtree(temp_region, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
