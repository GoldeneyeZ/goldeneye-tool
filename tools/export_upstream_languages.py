#!/usr/bin/env python3
"""Export codebase-memory-mcp's audited language tables as stable TSV."""

from __future__ import annotations

import argparse
import ast
import re
from collections import defaultdict
from pathlib import Path

UPSTREAM_REPOSITORY = "https://github.com/DeusData/codebase-memory-mcp"
UPSTREAM_COMMIT = "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c"
EXPECTED_COUNTS = (160, 239, 33, 1)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def table_body(source: str, name: str) -> str:
    declaration = re.search(rf"\b{name}\s*\[[^]]*]\s*=\s*\{{", source)
    if declaration is None:
        raise ValueError(f"table {name} not found")

    start = declaration.end()
    end = source.find("};", start)
    if end < 0:
        raise ValueError(f"table {name} has no closing initializer")
    return source[start:end]


def parse_enum_order(header: str) -> list[str]:
    match = re.search(
        r"typedef\s+enum\s*\{(?P<body>.*?)\}\s*CBMLanguage\s*;",
        header,
        re.DOTALL,
    )
    if match is None:
        raise ValueError("CBMLanguage enum not found")

    enum_order = re.findall(r"^\s*(CBM_LANG_[A-Z0-9_]+)\b", match.group("body"), re.MULTILINE)
    try:
        start = enum_order.index("CBM_LANG_GO")
        end = enum_order.index("CBM_LANG_COUNT")
    except ValueError as error:
        raise ValueError("CBMLanguage enum bounds not found") from error
    return enum_order[start:end]


def decode_c_string(value: str) -> str:
    return ast.literal_eval(f'"{value}"')


def parse_display_names(source: str) -> dict[str, str]:
    body = table_body(source, "LANG_NAMES")
    return {
        language: decode_c_string(display_name)
        for language, display_name in re.findall(
            r"\[\s*(CBM_LANG_[A-Z0-9_]+)\s*]\s*=\s*\"((?:\\.|[^\"\\])*)\"",
            body,
        )
    }


def parse_mapping_table(source: str, name: str) -> list[tuple[str, str]]:
    body = table_body(source, name)
    return [
        (decode_c_string(key), language)
        for key, language in re.findall(
            r"\{\s*\"((?:\\.|[^\"\\])*)\"\s*,\s*(CBM_LANG_[A-Z0-9_]+)\s*}",
            body,
        )
    ]


def normalized_id(enum_name: str) -> str:
    return enum_name.removeprefix("CBM_LANG_").lower()


def validate(
    enum_order: list[str],
    display_names: dict[str, str],
    extensions: list[tuple[str, str]],
    filenames: list[tuple[str, str]],
    compounds: list[tuple[str, str]],
) -> None:
    actual_counts = (len(enum_order), len(extensions), len(filenames), len(compounds))
    if actual_counts != EXPECTED_COUNTS:
        raise ValueError(
            "unexpected upstream counts: "
            f"languages/extensions/filenames/compounds={actual_counts}, expected={EXPECTED_COUNTS}"
        )

    enum_set = set(enum_order)
    if set(display_names) != enum_set:
        missing = sorted(enum_set - set(display_names))
        extra = sorted(set(display_names) - enum_set)
        raise ValueError(f"display-name mismatch: missing={missing}, extra={extra}")

    unknown = sorted(
        {language for _, language in extensions + filenames + compounds} - enum_set
    )
    if unknown:
        raise ValueError(f"mapping tables reference unknown languages: {unknown}")


def render(
    enum_order: list[str],
    display_names: dict[str, str],
    extensions: list[tuple[str, str]],
    filenames: list[tuple[str, str]],
    compounds: list[tuple[str, str]],
) -> str:
    extensions_by_language: dict[str, list[str]] = defaultdict(list)
    filenames_by_language: dict[str, list[str]] = defaultdict(list)
    compounds_by_language: dict[str, list[str]] = defaultdict(list)
    for extension, language in extensions:
        extensions_by_language[language].append(extension)
    for filename, language in filenames:
        filenames_by_language[language].append(filename)
    for extension, language in compounds:
        compounds_by_language[language].append(extension)

    lines = [
        f"# Derived from {UPSTREAM_REPOSITORY}",
        f"# Upstream commit: {UPSTREAM_COMMIT}",
        "# License: MIT; derived data preserves upstream provenance and notice.",
        "id\tdisplay_name\textensions\tfilenames\tcompound_extensions",
    ]
    for language in enum_order:
        lines.append(
            "\t".join(
                [
                    normalized_id(language),
                    display_names[language],
                    ",".join(extensions_by_language[language]),
                    ",".join(filenames_by_language[language]),
                    ",".join(compounds_by_language[language]),
                ]
            )
        )
    return "\n".join(lines) + "\n"


def export(upstream: Path, output: Path) -> None:
    header = (upstream / "internal/cbm/cbm.h").read_text(encoding="utf-8")
    language_source = (upstream / "src/discover/language.c").read_text(encoding="utf-8")

    enum_order = parse_enum_order(header)
    display_names = parse_display_names(language_source)
    extensions = parse_mapping_table(language_source, "EXT_TABLE")
    filenames = parse_mapping_table(language_source, "FILENAME_TABLE")
    compounds = parse_mapping_table(language_source, "COMPOUND_EXT_TABLE")
    validate(enum_order, display_names, extensions, filenames, compounds)

    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8", newline="\n") as destination:
        destination.write(
            render(enum_order, display_names, extensions, filenames, compounds)
        )


def main() -> None:
    arguments = parse_arguments()
    export(arguments.upstream.resolve(), arguments.output.resolve())


if __name__ == "__main__":
    main()
