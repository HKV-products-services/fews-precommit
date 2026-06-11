"""Validate Delft-FEWS XML configuration files against their XSD schemas.

Each staged file declares its schema in the root element's xsi:schemaLocation
(or xsi:noNamespaceSchemaLocation). The schema is resolved in this order:

1. by file name in a local schema directory (e.g. "explorer.xsd" in
   Schemas/), so a whole FEWS configuration can be validated without
   internet access;
2. a relative hint (e.g. "../xsd/rtcDataConfig.xsd") is resolved against
   the XML file's own location;
3. with --allow-remote, an http(s) hint is downloaded.

Files that declare no schema are skipped.

Part of fews-precommit; installed by pre-commit as the fews-xsd-validate hook.
"""

from __future__ import annotations

import argparse
import sys
import textwrap
import xml.etree.ElementTree as ET
from pathlib import Path
from urllib.parse import urlparse

import xmlschema

WRAP = textwrap.TextWrapper(width=88, initial_indent="    ", subsequent_indent="    ")

XSI = "{http://www.w3.org/2001/XMLSchema-instance}"


def schema_hint(xml_file: Path) -> str | None:
    """Return the schema location declared on the root element, or None.

    xsi:schemaLocation holds (namespace, location) pairs; FEWS files declare
    exactly one pair, so the last token is the location.
    """
    for _event, element in ET.iterparse(xml_file, events=("start",)):
        location = element.get(XSI + "schemaLocation")
        if location and location.split():
            return location.split()[-1]
        return element.get(XSI + "noNamespaceSchemaLocation")
    return None


def schema_file_name(hint: str) -> str:
    # Hints written on Windows may use backslashes; normalize so the file
    # name (and relative resolution) also works on Linux/macOS.
    return Path(urlparse(hint.replace("\\", "/")).path).name


def resolve_schema(
    hint: str, xml_file: Path, schema_dir: Path, allow_remote: bool
) -> str | None:
    """Map a schemaLocation hint to an XSD source.

    A pinned copy in schema_dir wins; a relative hint is resolved against
    the XML file's directory; an http(s) hint is used directly when
    --allow-remote is given.
    """
    local = schema_dir / schema_file_name(hint)
    if local.is_file():
        return str(local)
    if urlparse(hint).scheme in ("http", "https"):
        return hint if allow_remote else None
    relative = xml_file.parent / hint.replace("\\", "/")
    if relative.is_file():
        return str(relative)
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="fews-xsd-validate",
        description="Validate Delft-FEWS XML files against their XSD schemas.",
    )
    parser.add_argument("files", nargs="*", type=Path, help="XML files to validate")
    parser.add_argument(
        "--schema-dir",
        type=Path,
        default=Path("Schemas"),
        help="directory with local copies of the FEWS XSD schemas (default: Schemas)",
    )
    parser.add_argument(
        "--allow-remote",
        action="store_true",
        help="download the XSD from the file's schemaLocation URL when it is "
        "not in the schema directory (requires internet access on commit)",
    )
    parser.add_argument(
        "--missing-schema",
        choices=("warn", "skip", "fail"),
        default="warn",
        help="what to do when a declared schema is not available locally "
        "(default: warn, which reports but does not fail)",
    )
    args = parser.parse_args(argv)

    schemas: dict[str, xmlschema.XMLSchemaBase] = {}
    invalid = 0
    errors = 0

    for xml_file in args.files:
        try:
            hint = schema_hint(xml_file)
        except (ET.ParseError, OSError) as exc:
            print(f"{xml_file}: cannot read: {exc}")
            errors += 1
            continue
        if hint is None:
            continue  # no schema declared, nothing to validate against

        source = resolve_schema(hint, xml_file, args.schema_dir, args.allow_remote)
        if source is None:
            name = schema_file_name(hint)
            remote_helps = (
                not args.allow_remote and urlparse(hint).scheme in ("http", "https")
            )
            tip = f"add '{name}' to {args.schema_dir}" + (
                " or pass --allow-remote" if remote_helps else ""
            )
            if args.missing_schema == "fail":
                print(f"{xml_file}: schema not found (declared: {hint}); {tip}")
                errors += 1
            elif args.missing_schema == "warn":
                print(f"{xml_file}: not validated, schema not found; {tip}")
            continue

        try:
            schema = schemas.get(source)
            if schema is None:
                schema = xmlschema.XMLSchema(source)
                schemas[source] = schema
            file_errors = list(schema.iter_errors(str(xml_file)))
        except (xmlschema.XMLSchemaException, ET.ParseError, OSError) as exc:
            print(f"{xml_file}: {exc}")
            errors += 1
            continue

        if file_errors:
            invalid += 1
            count = "1 error" if len(file_errors) == 1 else f"{len(file_errors)} errors"
            print(f"{xml_file}: {count}")
            for error in file_errors:
                print(f"  at {error.path or '/'}:")
                reason = " ".join((error.reason or str(error)).split())
                print(WRAP.fill(reason))

    if errors:
        return 2
    return 1 if invalid else 0


if __name__ == "__main__":
    sys.exit(main())
