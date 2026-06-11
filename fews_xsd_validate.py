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

Validation runs on lxml (libxml2, the C library behind xmllint), so large
FEWS schemas compile and validate at native speed. Schemas that strict
libxml2 rejects (e.g. a schemaLocation with a stray trailing space, which
occurs in shipped FEWS schemas) are handled by the tolerant pure-Python
xmlschema engine instead.

Part of fews-precommit; installed by pre-commit as the fews-xsd-validate hook.
"""

from __future__ import annotations

import argparse
import sys
import textwrap
from io import BytesIO
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import urlopen

from lxml import etree

XSI = "http://www.w3.org/2001/XMLSchema-instance"
MAX_ERRORS_SHOWN = 10
WRAP = textwrap.TextWrapper(width=88, initial_indent="    ", subsequent_indent="    ")


def schema_hint(root: etree._Element) -> str | None:
    """Return the schema location declared on the root element, or None.

    xsi:schemaLocation holds (namespace, location) pairs; FEWS files declare
    exactly one pair, so the last token is the location.
    """
    location = root.get(f"{{{XSI}}}schemaLocation")
    if location and location.split():
        return location.split()[-1]
    return root.get(f"{{{XSI}}}noNamespaceSchemaLocation")


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


class SchemaLoadError(Exception):
    pass


def load_schema(source: str):
    """Compile a schema with lxml; fall back to the tolerant pure-Python
    xmlschema engine when strict libxml2 rejects the schema itself."""
    try:
        if urlparse(source).scheme in ("http", "https"):
            # Reached only via --allow-remote.
            with urlopen(source) as response:  # noqa: S310
                data = response.read()
            document = etree.parse(BytesIO(data), base_url=source)
        else:
            document = etree.parse(source)
        return etree.XMLSchema(document)
    except etree.XMLSchemaParseError as lxml_error:
        try:
            import xmlschema
        except ImportError:
            raise SchemaLoadError(str(lxml_error)) from lxml_error
        try:
            return xmlschema.XMLSchema(source)
        except Exception as fallback_error:
            raise SchemaLoadError(
                f"{lxml_error} (pure-Python fallback also failed: {fallback_error})"
            ) from fallback_error
    except (etree.LxmlError, OSError) as exc:
        raise SchemaLoadError(str(exc)) from exc


def validation_errors(schema, xml_file: Path, tree) -> list[tuple[str, str]]:
    """Returns (location, message) pairs; empty when the file is valid."""
    if isinstance(schema, etree.XMLSchema):
        if schema.validate(tree):
            return []
        return [
            (f"line {entry.line}", " ".join(entry.message.split()))
            for entry in schema.error_log
        ]
    return [
        (entry.path or "/", " ".join((entry.reason or str(entry)).split()))
        for entry in schema.iter_errors(str(xml_file))
    ]


def report_errors(xml_file: Path, found: list[tuple[str, str]]) -> None:
    count = "1 error" if len(found) == 1 else f"{len(found)} errors"
    print(f"{xml_file}: {count}")
    for location, message in found[:MAX_ERRORS_SHOWN]:
        print(f"  at {location}:")
        print(WRAP.fill(message))
    if len(found) > MAX_ERRORS_SHOWN:
        print(f"  ... and {len(found) - MAX_ERRORS_SHOWN} more")


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
        "not available locally (requires internet access on commit)",
    )
    parser.add_argument(
        "--missing-schema",
        choices=("warn", "skip", "fail"),
        default="warn",
        help="what to do when a declared schema is not available locally "
        "(default: warn, which reports but does not fail)",
    )
    args = parser.parse_args(argv)

    xml_parser = etree.XMLParser(resolve_entities=False, huge_tree=True)
    schemas: dict[str, object] = {}
    invalid = 0
    errors = 0

    for xml_file in args.files:
        try:
            tree = etree.parse(str(xml_file), xml_parser)
        except (etree.LxmlError, OSError) as exc:
            print(f"{xml_file}: cannot read: {exc}")
            errors += 1
            continue

        hint = schema_hint(tree.getroot())
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
                schema = load_schema(source)
                schemas[source] = schema
        except SchemaLoadError as exc:
            print(f"{xml_file}: cannot load schema {source}: {exc}")
            errors += 1
            continue

        found = validation_errors(schema, xml_file, tree)
        if found:
            invalid += 1
            report_errors(xml_file, found)

    if errors:
        return 2
    return 1 if invalid else 0


if __name__ == "__main__":
    sys.exit(main())
