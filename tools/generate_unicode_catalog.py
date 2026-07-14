#!/usr/bin/env python3
"""Génère les catalogues hors ligne ClipH depuis Unicode 17 et CLDR 48.2."""

from __future__ import annotations

import bisect
import io
import re
import sys
import time
import urllib.request
import zipfile
from pathlib import Path
from xml.etree import ElementTree

UNICODE_VERSION = "17.0.0"
EMOJI_VERSION = "17.0"
CLDR_VERSION = "48.2"

EMOJI_URL = (
    "https://www.unicode.org/Public/emoji/latest/emoji-test.txt"
)
CLDR_URL = (
    f"https://unicode.org/Public/cldr/{CLDR_VERSION}/"
    f"cldr-common-{CLDR_VERSION}.zip"
)
UNICODE_DATA_URL = (
    f"https://unicode.org/Public/{UNICODE_VERSION}/ucd/UnicodeData.txt"
)
BLOCKS_URL = (
    f"https://unicode.org/Public/{UNICODE_VERSION}/ucd/Blocks.txt"
)
LICENSE_URL = "https://www.unicode.org/license.txt"

ROOT = Path.cwd()
CACHE_DIR = ROOT / ".cache" / "cliph-unicode"
DATA_DIR = ROOT / "crates" / "cliph-core" / "data"
THIRD_PARTY_DIR = ROOT / "third-party" / "unicode"

EMOJI_FILE = CACHE_DIR / f"emoji-test-{EMOJI_VERSION}.txt"
CLDR_FILE = CACHE_DIR / f"cldr-common-{CLDR_VERSION}.zip"
UNICODE_DATA_FILE = CACHE_DIR / f"UnicodeData-{UNICODE_VERSION}.txt"
BLOCKS_FILE = CACHE_DIR / f"Blocks-{UNICODE_VERSION}.txt"
LICENSE_FILE = THIRD_PARTY_DIR / "LICENSE.txt"

EMOJI_GROUPS_FR = {
    "Smileys & Emotion": "Visages et émotions",
    "People & Body": "Personnes et corps",
    "Animals & Nature": "Animaux et nature",
    "Food & Drink": "Nourriture et boissons",
    "Travel & Places": "Voyages et lieux",
    "Activities": "Activités",
    "Objects": "Objets",
    "Symbols": "Symboles",
    "Flags": "Drapeaux",
    "Component": "Composants",
}

EMOJI_ALIASES = {
    "📎": ["piece jointe", "fichier joint", "attache"],
    "🥳": ["celebration", "fete", "anniversaire"],
    "✅": ["valider", "validation", "termine", "fait"],
    "❌": ["erreur", "annuler", "refuser"],
    "⚠️": ["attention", "avertissement", "danger"],
    "💡": ["idee", "astuce"],
    "🚀": ["lancement", "projet", "demarrage"],
    "🎯": ["objectif", "cible"],
    "📅": ["calendrier", "date", "rendez vous"],
    "📧": ["courriel", "email", "mail"],
    "🔒": ["securite", "prive", "verrouille"],
    "🔓": ["deverrouille", "ouvert", "public"],
}

FRENCH_NAME_WORDS = {
    "LEFT": "gauche",
    "RIGHT": "droite",
    "UP": "haut",
    "DOWN": "bas",
    "ARROW": "fleche",
    "ARROWS": "fleches",
    "CIRCLE": "cercle",
    "SQUARE": "carre",
    "TRIANGLE": "triangle",
    "DIAMOND": "losange",
    "STAR": "etoile",
    "BLACK": "noir",
    "WHITE": "blanc",
    "PLUS": "plus",
    "MINUS": "moins",
    "EQUAL": "egal",
    "EQUALS": "egal",
    "LESS-THAN": "inferieur",
    "GREATER-THAN": "superieur",
    "INTEGRAL": "integrale",
    "SUMMATION": "somme",
    "PRODUCT": "produit",
    "ROOT": "racine",
    "INFINITY": "infini",
    "DEGREE": "degre",
    "COPYRIGHT": "copyright droit auteur",
    "REGISTERED": "marque deposee",
    "TRADE": "commerce",
    "MARK": "marque",
    "CHECK": "validation coche",
    "CROSS": "croix",
    "MUSIC": "musique",
    "MUSICAL": "musique",
    "NOTE": "note",
    "NOTES": "notes",
    "GREEK": "grec",
    "LETTER": "lettre",
    "CURRENCY": "monnaie",
    "DOLLAR": "dollar",
    "EURO": "euro",
    "YEN": "yen",
    "POUND": "livre",
    "CENT": "centime",
    "SECTION": "section",
    "PARAGRAPH": "paragraphe",
    "BULLET": "puce",
    "DASH": "tiret",
    "QUOTATION": "guillemet",
    "OPEN": "ouvrant",
    "CLOSE": "fermant",
    "HEART": "coeur",
    "SUN": "soleil",
    "MOON": "lune",
    "EARTH": "terre",
    "FEMALE": "femme",
    "MALE": "homme",
    "PHONE": "telephone",
    "KEYBOARD": "clavier",
    "RETURN": "entree retour",
    "DELETE": "supprimer",
}

SYMBOL_ALIASES = {
    "≤": ["inferieur egal", "plus petit ou egal"],
    "≥": ["superieur egal", "plus grand ou egal"],
    "≠": ["different", "inegalite"],
    "≈": ["environ", "approximativement egal"],
    "∞": ["infini"],
    "√": ["racine carree"],
    "∑": ["somme sigma"],
    "∫": ["integrale"],
    "→": ["fleche droite", "suivant"],
    "←": ["fleche gauche", "retour"],
    "↑": ["fleche haut", "monter"],
    "↓": ["fleche bas", "descendre"],
    "€": ["euro monnaie argent"],
    "₦": ["naira nigeria monnaie"],
    "©": ["copyright droit auteur"],
    "®": ["marque deposee"],
    "™": ["marque commerciale"],
    "°": ["degre temperature angle"],
    "…": ["points de suspension ellipse"],
    "«": ["guillemet francais ouvrant"],
    "»": ["guillemet francais fermant"],
}

PUNCTUATION_CATEGORIES = {
    "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps"
}

SKIP_NAME_PARTS = {
    "VARIATION SELECTOR",
    "COMBINING",
    "ZERO WIDTH",
    "BYTE ORDER MARK",
    "OBJECT REPLACEMENT CHARACTER",
    "REPLACEMENT CHARACTER",
    "INTERLINEAR ANNOTATION",
    "INVISIBLE",
    "TAG ",
}


def download(url: str, destination: Path) -> None:
    if destination.exists() and destination.stat().st_size > 0:
        print(f"Cache utilisé : {destination}")
        return

    destination.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "ClipH Unicode catalog generator/1.0"},
    )

    last_error: Exception | None = None

    for attempt in range(1, 4):
        try:
            print(f"Téléchargement : {url}")
            with urllib.request.urlopen(request, timeout=90) as response:
                data = response.read()
            destination.write_bytes(data)
            print(f"  {len(data):,} octets enregistrés")
            return
        except Exception as error:
            last_error = error
            print(f"  tentative {attempt}/3 échouée : {error}")
            if attempt < 3:
                time.sleep(attempt * 2)

    raise RuntimeError(
        f"Impossible de télécharger {url}: {last_error}"
    )


def clean_field(value: str) -> str:
    return " ".join(
        value.replace("\t", " ")
        .replace("\r", " ")
        .replace("\n", " ")
        .replace("|", " ")
        .split()
    )


def unique_words(values: list[str]) -> list[str]:
    result: list[str] = []
    seen: set[str] = set()

    for value in values:
        cleaned = clean_field(value).strip()
        if not cleaned:
            continue
        key = cleaned.casefold()
        if key not in seen:
            seen.add(key)
            result.append(cleaned)

    return result


def parse_cldr_annotations(
    archive_path: Path,
) -> tuple[dict[str, str], dict[str, list[str]]]:
    labels: dict[str, str] = {}
    keywords: dict[str, list[str]] = {}

    with zipfile.ZipFile(archive_path) as archive:
        paths = [
            "common/annotations/fr.xml",
            "common/annotationsDerived/fr.xml",
        ]

        for path in paths:
            print(f"Lecture CLDR : {path}")
            with archive.open(path) as stream:
                root = ElementTree.parse(stream).getroot()

            for annotation in root.iter("annotation"):
                character = annotation.attrib.get("cp")
                text = annotation.text

                if not character or not text or text == "↑↑↑":
                    continue

                if annotation.attrib.get("type") == "tts":
                    labels[character] = clean_field(text)
                else:
                    keywords.setdefault(character, []).extend(
                        part.strip()
                        for part in text.split("|")
                        if part.strip()
                    )

    return labels, keywords


def cldr_value(
    mapping: dict[str, object],
    value: str,
):
    candidates = [
        value,
        value.replace("\ufe0f", ""),
    ]

    for candidate in candidates:
        if candidate in mapping:
            return mapping[candidate]

    return None


def parse_emoji_catalog(
    emoji_path: Path,
    labels: dict[str, str],
    cldr_keywords: dict[str, list[str]],
) -> tuple[list[tuple[str, str, str, list[str]]], set[str]]:
    entries: list[tuple[str, str, str, list[str]]] = []
    values: set[str] = set()
    current_group = "Autres émojis"
    current_subgroup = ""

    line_pattern = re.compile(
        r"^([0-9A-F ]+)\s*;\s*([a-z-]+)\s*#\s*"
        r"\S+\s+E[\d.]+\s+(.+)$"
    )

    for raw_line in emoji_path.read_text(encoding="utf-8").splitlines():
        if raw_line.startswith("# group: "):
            group_en = raw_line.removeprefix("# group: ").strip()
            current_group = EMOJI_GROUPS_FR.get(group_en, group_en)
            continue

        if raw_line.startswith("# subgroup: "):
            current_subgroup = (
                raw_line.removeprefix("# subgroup: ")
                .strip()
                .replace("-", " ")
            )
            continue

        match = line_pattern.match(raw_line)
        if not match:
            continue

        codepoints_text, status, english_name = match.groups()

        if status not in {"fully-qualified", "component"}:
            continue

        value = "".join(
            chr(int(codepoint, 16))
            for codepoint in codepoints_text.split()
        )

        if value in values:
            continue

        label = cldr_value(labels, value) or clean_field(english_name)
        keywords = list(cldr_value(cldr_keywords, value) or [])
        keywords.extend(
            [
                english_name,
                current_subgroup,
                current_group,
            ]
        )
        keywords.extend(EMOJI_ALIASES.get(value, []))

        entries.append(
            (
                current_group,
                value,
                str(label),
                unique_words(keywords),
            )
        )
        values.add(value)

    return entries, values


def parse_blocks(path: Path) -> tuple[list[int], list[tuple[int, str]]]:
    starts: list[int] = []
    records: list[tuple[int, str]] = []

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue

        range_text, block_name = (
            part.strip() for part in line.split(";", 1)
        )
        start_text, end_text = range_text.split("..", 1)
        start = int(start_text, 16)
        end = int(end_text, 16)

        starts.append(start)
        records.append((end, block_name))

    return starts, records


def find_block(
    codepoint: int,
    starts: list[int],
    records: list[tuple[int, str]],
) -> str:
    index = bisect.bisect_right(starts, codepoint) - 1

    if index < 0:
        return "Autres"

    end, name = records[index]
    if codepoint <= end:
        return name

    return "Autres"


def symbol_group(block: str, category: str) -> str:
    uppercase = block.upper()

    if category == "Sc" or "CURRENCY" in uppercase:
        return "Monnaies"
    if "ARROW" in uppercase:
        return "Flèches"
    if category == "Sm" or any(
        token in uppercase
        for token in (
            "MATHEMATICAL",
            "NUMBER FORMS",
            "SUPERSCRIPTS",
            "SUBSCRIPTS",
        )
    ):
        return "Mathématiques"
    if category in PUNCTUATION_CATEGORIES:
        return "Ponctuation et typographie"
    if "GREEK" in uppercase:
        return "Lettres grecques"
    if any(
        token in uppercase
        for token in ("GEOMETRIC", "SHAPES")
    ):
        return "Formes géométriques"
    if any(
        token in uppercase
        for token in ("BOX DRAWING", "BLOCK ELEMENTS")
    ):
        return "Boîtes et blocs"
    if "BRAILLE" in uppercase:
        return "Braille"
    if "MUSICAL" in uppercase:
        return "Musique"
    if any(
        token in uppercase
        for token in ("CHESS", "CARDS", "DOMINO", "MAHJONG")
    ):
        return "Jeux"
    if "DINGBATS" in uppercase:
        return "Dingbats"
    if any(
        token in uppercase
        for token in (
            "TECHNICAL",
            "CONTROL PICTURES",
            "OPTICAL",
        )
    ):
        return "Technique"
    if "ALCHEMICAL" in uppercase:
        return "Alchimie"
    if any(
        token in uppercase
        for token in (
            "ASTROLOGICAL",
            "MISCELLANEOUS SYMBOLS",
        )
    ):
        return "Nature, météo et astronomie"

    return "Autres symboles"


def french_name_keywords(name: str) -> list[str]:
    keywords: list[str] = []

    for token, translation in FRENCH_NAME_WORDS.items():
        if token in name:
            keywords.append(translation)

    return keywords


def should_include_symbol(
    category: str,
    block: str,
    name: str,
) -> bool:
    if any(part in name for part in SKIP_NAME_PARTS):
        return False

    if category.startswith("S"):
        return True

    if category in PUNCTUATION_CATEGORIES:
        return True

    if category in {"Nl", "No"} and any(
        token in block.upper()
        for token in (
            "NUMBER FORMS",
            "SUPERSCRIPTS",
            "SUBSCRIPTS",
        )
    ):
        return True

    return False


def parse_symbol_catalog(
    unicode_data_path: Path,
    blocks_path: Path,
    emoji_values: set[str],
) -> list[tuple[str, str, str, list[str]]]:
    starts, blocks = parse_blocks(blocks_path)
    entries: list[tuple[str, str, str, list[str]]] = []
    seen: set[str] = set()

    for raw_line in unicode_data_path.read_text(
        encoding="utf-8"
    ).splitlines():
        fields = raw_line.split(";")
        if len(fields) < 3:
            continue

        codepoint = int(fields[0], 16)
        name = fields[1]
        category = fields[2]

        if name.startswith("<"):
            continue

        block = find_block(codepoint, starts, blocks)

        if not should_include_symbol(category, block, name):
            continue

        value = chr(codepoint)

        if value in seen or value in emoji_values:
            continue

        if value.isspace() or not value.isprintable():
            continue

        group = symbol_group(block, category)
        label = name.title().replace("-", " ")
        keywords = [
            name,
            block,
            category,
            *french_name_keywords(name),
            *SYMBOL_ALIASES.get(value, []),
        ]

        entries.append(
            (group, value, label, unique_words(keywords))
        )
        seen.add(value)

    entries.sort(key=lambda item: (item[0], ord(item[1])))
    return entries


def write_tsv(
    path: Path,
    entries: list[tuple[str, str, str, list[str]]],
    header: str,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)

    with path.open("w", encoding="utf-8", newline="\n") as output:
        output.write(f"# {header}\n")
        for group, value, label, keywords in entries:
            output.write(
                "\t".join(
                    [
                        clean_field(group),
                        clean_field(value),
                        clean_field(label),
                        "|".join(unique_words(keywords)),
                    ]
                )
                + "\n"
            )


def main() -> int:
    required = ROOT / "crates" / "cliph-core" / "src"
    if not required.is_dir():
        print(
            "Exécutez ce script depuis la racine du projet ClipH.",
            file=sys.stderr,
        )
        return 1

    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    THIRD_PARTY_DIR.mkdir(parents=True, exist_ok=True)

    download(EMOJI_URL, EMOJI_FILE)
    download(CLDR_URL, CLDR_FILE)
    download(UNICODE_DATA_URL, UNICODE_DATA_FILE)
    download(BLOCKS_URL, BLOCKS_FILE)
    download(LICENSE_URL, LICENSE_FILE)

    labels, cldr_keywords = parse_cldr_annotations(CLDR_FILE)

    emoji_entries, emoji_values = parse_emoji_catalog(
        EMOJI_FILE,
        labels,
        cldr_keywords,
    )

    symbol_entries = parse_symbol_catalog(
        UNICODE_DATA_FILE,
        BLOCKS_FILE,
        emoji_values,
    )

    write_tsv(
        DATA_DIR / "emoji.tsv",
        emoji_entries,
        (
            f"Unicode Emoji {EMOJI_VERSION} + "
            f"CLDR {CLDR_VERSION} français"
        ),
    )
    write_tsv(
        DATA_DIR / "symbols.tsv",
        symbol_entries,
        f"Unicode {UNICODE_VERSION} symboles imprimables",
    )

    metadata = (
        f"unicode={UNICODE_VERSION}\n"
        f"emoji={EMOJI_VERSION}\n"
        f"cldr={CLDR_VERSION}\n"
        f"emoji_count={len(emoji_entries)}\n"
        f"symbol_count={len(symbol_entries)}\n"
    )
    (DATA_DIR / "catalog-versions.txt").write_text(
        metadata,
        encoding="utf-8",
    )

    print()
    print("Catalogue ClipH généré avec succès :")
    print(f"  Émojis   : {len(emoji_entries):,}")
    print(f"  Symboles : {len(symbol_entries):,}")
    print(f"  Dossier  : {DATA_DIR}")

    if len(emoji_entries) < 3_500:
        raise RuntimeError(
            "Le catalogue d’émojis paraît incomplet."
        )

    if len(symbol_entries) < 5_000:
        raise RuntimeError(
            "Le catalogue de symboles paraît incomplet."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
