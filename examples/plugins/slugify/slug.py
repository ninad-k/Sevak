"""Slugs and text statistics for the "slug" keyword (Sevak one-shot plugin).

"slug Hello, World! Café" gives a URL-safe slug (hello-world-cafe), the same
with underscores, and a filename-safe form. It also counts characters, words,
lines and bytes, and estimates reading time. Accents are folded to plain
letters. Standard library only; nothing is stored or sent anywhere.
"""
import json
import re
import sys
import unicodedata

WORDS_PER_MINUTE = 238
MAX_LENGTH = 5000


def fold(text):
    """Letters without accents; characters with no plain form are dropped."""
    text = text.replace("ß", "ss").replace("æ", "ae").replace("Æ", "AE")
    text = text.replace("ø", "o").replace("Ø", "O").replace("ł", "l").replace("Ł", "L")
    decomposed = unicodedata.normalize("NFKD", text)
    return "".join(c for c in decomposed if not unicodedata.combining(c))


def slug(text, separator="-"):
    words = re.findall(r"[a-z0-9]+", fold(text).lower())
    return separator.join(words)


def filename(text):
    cleaned = re.sub(r"[^A-Za-z0-9._ -]+", "", fold(text)).strip(" .")
    return re.sub(r"\s+", "-", cleaned) or "untitled"


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title[:200],
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def results(query):
    text = query.strip()
    if not text:
        return [row("hint", "Type or paste some text",
                    "slug My First Blog Post! gives my-first-blog-post, plus word and character counts", "my-first-blog-post")]
    if len(text) > MAX_LENGTH:
        return [row("long", "Text is longer than 5000 characters", "Paste a shorter piece", "")]
    words = len(text.split())
    minutes = words / WORDS_PER_MINUTE
    reading = "under a minute" if minutes < 1 else f"about {round(minutes)} minute{'' if round(minutes) == 1 else 's'}"
    full = slug(text)
    return [
        row("slug", full or "(nothing left after cleaning)", "URL slug. Enter to copy", full),
        row("snake", slug(text, "_") or "(nothing left)", "With underscores", slug(text, "_")),
        row("short", "-".join(full.split("-")[:6]) or "(nothing left)", "First six words only, for short URLs", "-".join(full.split("-")[:6])),
        row("filename", filename(text), "Safe as a file name on Windows, macOS and Linux"),
        row("chars", f"{len(text)} characters", f"{len(text.replace(' ', '').replace(chr(10), ''))} without spaces", str(len(text))),
        row("words", f"{words} word{'' if words == 1 else 's'}", f"{len(text.splitlines())} line(s). Reading time {reading}", str(words)),
        row("bytes", f"{len(text.encode('utf-8'))} bytes in UTF-8", "Size of the text when saved or sent", str(len(text.encode("utf-8")))),
    ]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
