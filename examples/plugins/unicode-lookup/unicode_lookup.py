"""Unicode character lookup for the "uni" keyword (Sevak one-shot plugin).

"uni U+1F600" or "uni 0x2192" looks up a code point, "uni é" (a character you
pasted) describes it, and "uni right arrow" searches character names. Uses the
Unicode data that ships with Python. Standard library only; nothing is stored
or sent anywhere.
"""
import json
import re
import sys
import unicodedata

CODE = re.compile(r"(?:u\+|0x|\\u\{?)([0-9a-f]{2,6})\}?", re.IGNORECASE)
SEARCH_LIMIT = 0x20000  # the Basic Multilingual Plane and the next plane (emoji live here)
MAX_ROWS = 12
CATEGORIES = {
    "Lu": "uppercase letter", "Ll": "lowercase letter", "Lt": "titlecase letter", "Lm": "modifier letter",
    "Lo": "other letter", "Mn": "nonspacing mark", "Mc": "spacing mark", "Me": "enclosing mark",
    "Nd": "decimal digit", "Nl": "letter number", "No": "other number", "Pc": "connector punctuation",
    "Pd": "dash", "Ps": "opening bracket", "Pe": "closing bracket", "Pi": "initial quote",
    "Pf": "final quote", "Po": "other punctuation", "Sm": "math symbol", "Sc": "currency symbol",
    "Sk": "modifier symbol", "So": "other symbol", "Zs": "space", "Zl": "line separator",
    "Zp": "paragraph separator", "Cc": "control character", "Cf": "format character",
    "Cs": "surrogate", "Co": "private use", "Cn": "unassigned",
}


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title[:200],
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def escapes(char):
    code = ord(char)
    return code, [
        ("char", char if char.isprintable() and not char.isspace() else f"U+{code:04X}", "The character. Enter to copy", char),
        ("code", f"U+{code:04X}", "Code point"),
        ("utf8", " ".join(f"{b:02X}" for b in char.encode("utf-8", "surrogatepass")), "UTF-8 bytes"),
        ("html", f"&#x{code:X};", "HTML entity"),
        ("js", f"\\u{{{code:X}}}", "JavaScript and Rust escape"),
        ("py", f"\\U{code:08x}" if code > 0xFFFF else f"\\u{code:04x}", "Python escape"),
        ("dec", str(code), "Decimal code point"),
    ]


def describe(char):
    code = ord(char)
    name = unicodedata.name(char, None) or ("<control>" if unicodedata.category(char) == "Cc" else "(no name)")
    category = unicodedata.category(char)
    items = [row(f"name-{code:X}", name, f"U+{code:04X}, {CATEGORIES.get(category, category)}. Enter to copy the name")]
    for key, title, subtitle, *copy in escapes(char)[1]:
        items.append(row(f"{key}-{code:X}", title, subtitle, copy[0] if copy else None))
    decomposition = unicodedata.normalize("NFD", char)
    if decomposition != char:
        parts = " + ".join(f"U+{ord(c):04X}" for c in decomposition)
        items.append(row(f"nfd-{code:X}", parts, "Decomposed (NFD): base letter and combining marks", decomposition))
    return items


def search(words):
    words = [w.upper() for w in words]
    hits = []
    for code in range(0x20, SEARCH_LIMIT):
        name = unicodedata.name(chr(code), "")
        if name and all(w in name for w in words):
            hits.append((len(name), code, name))
    hits.sort()
    return hits[:MAX_ROWS]


def results(query):
    text = query.strip()
    if not text:
        return [row("hint", "Type a character, U+1F600 or a name like \"right arrow\"",
                    "Looks up code points, UTF-8 bytes, HTML entities and escapes", "U+2192")]
    match = CODE.fullmatch(text)
    if match:
        code = int(match.group(1), 16)
        if code > 0x10FFFF:
            return [row("bad", "Code points end at U+10FFFF", "Try a smaller number", "U+10FFFF")]
        return describe(chr(code))
    if len(text) <= 6 and any(ord(c) > 127 for c in text) or len(text) == 1:
        chars = list(text)[:4]
        if len(chars) == 1:
            return describe(chars[0])
        return [row(f"c{i}", f"U+{ord(c):04X}  {unicodedata.name(c, '(no name)')}", "Enter to copy the code point", f"U+{ord(c):04X}")
                for i, c in enumerate(chars)]
    hits = search(text.replace("-", " ").split())
    if not hits:
        return [row("none", "No character with that name", "Try fewer words, for example: arrow, check, snowman", "U+2192")]
    return [row(f"hit-{code:X}", f"{chr(code)}  {name.title()}", f"U+{code:04X}. Enter to copy the character", chr(code))
            for _, code, name in hits]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
