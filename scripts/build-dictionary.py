#!/usr/bin/env python3
"""Builds crates/sevak-plugins/data/wordnet-en.z, the offline dictionary that
Sevak's `define` and `spell` keywords use where the OS has none.

Source: Princeton WordNet 3.0 (https://wordnet.princeton.edu), whose licence
allows redistribution of modified copies if the notice stays with them. The
notice is embedded in the data file's header and reproduced in
THIRD_PARTY_NOTICES.md. The output is derived data: run this only to
regenerate it.

    python scripts/build-dictionary.py /path/to/WordNet-3.0 [--phrases]

Output (zlib-compressed UTF-8 text):

    #<header lines: licence notice>
    <word> TAB <frequency> TAB <sense> [TAB <sense>]...      sorted by word

A sense is `<pos> <definition>` where pos is n, v, a (adjective) or r (adverb),
or `>lemma`, meaning "an inflected form of lemma" (WordNet's exception lists).
Example sentences are dropped. Only single words are kept unless --phrases.
"""
import re
import sys
import zlib
from pathlib import Path

MAX_SENSES_PER_POS = 3
MAX_GLOSS_CHARS = 160
POS_FILES = {"n": "noun", "v": "verb", "a": "adj", "r": "adv"}
WORD_OK = re.compile(r"^[a-z][a-z0-9'.\-]*$")


def read_data(path):
    glosses = {}
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("  ") or "|" not in line:
            continue
        head, gloss = line.split("|", 1)
        offset = head.split(" ", 1)[0]
        # Drop the usage examples ("; \"...\"") and keep the definition.
        definition = re.split(r';\s+"', gloss, maxsplit=1)[0].strip().rstrip(";").strip()
        definition = re.sub(r"\s+", " ", definition)
        if len(definition) > MAX_GLOSS_CHARS:
            definition = definition[: MAX_GLOSS_CHARS - 1].rstrip() + "…"
        glosses[offset] = definition
    return glosses


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    phrases = "--phrases" in sys.argv
    if len(args) != 1:
        sys.exit(__doc__)
    root = Path(args[0])
    dictdir = root / "dict"
    entries = {}  # word -> [freq, [senses]]
    for pos, name in POS_FILES.items():
        glosses = read_data(dictdir / f"data.{name}")
        for line in (dictdir / f"index.{name}").read_text(encoding="utf-8").splitlines():
            if line.startswith("  "):
                continue
            fields = line.split()
            word = fields[0]
            if "_" in word and not phrases:
                continue
            word = word.replace("_", " ")
            if not (WORD_OK.match(word.replace(" ", "")) and word.isascii()):
                continue
            synset_cnt = int(fields[2])
            pointer_cnt = int(fields[3])
            rest = fields[4 + pointer_cnt:]
            tagsense_cnt = int(rest[1])
            offsets = rest[2:2 + synset_cnt]
            entry = entries.setdefault(word, [0, []])
            entry[0] += tagsense_cnt
            for offset in offsets[:MAX_SENSES_PER_POS]:
                gloss = glosses.get(offset)
                if gloss:
                    entry[1].append(f"{pos} {gloss}")
    # Irregular forms: "children child". Only those whose base word is kept.
    for pos, name in POS_FILES.items():
        exc = dictdir / f"{name}.exc"
        if not exc.exists():
            continue
        for line in exc.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) < 2:
                continue
            if ("_" in parts[0] or "_" in parts[1]) and not phrases:
                continue
            form, base = parts[0].replace("_", " "), parts[1].replace("_", " ")
            if base not in entries or not WORD_OK.match(form.replace(" ", "")):
                continue
            if form in entries:
                continue
            entries[form] = [0, [f">{base}"]]

    license_text = (root / "LICENSE").read_text(encoding="utf-8")
    header = ["#WordNet 3.0 (trimmed by Sevak: single words, definitions only)"]
    header += ["#" + l.rstrip() for l in license_text.splitlines() if l.strip()]
    out = ["\n".join(header)]
    for word in sorted(entries):
        freq, senses = entries[word]
        if not senses or "\t" in word:
            continue
        out.append("\t".join([word, str(freq)] + senses))
    raw = ("\n".join(out) + "\n").encode("utf-8")
    packed = zlib.compress(raw, 9)
    target = Path(__file__).resolve().parent.parent / "crates/sevak-plugins/data/wordnet-en.z"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(packed)
    print(f"{len(entries)} words, {len(raw):,} bytes raw, {len(packed):,} bytes compressed -> {target}")


if __name__ == "__main__":
    main()
