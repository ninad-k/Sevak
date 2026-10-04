"""Random passwords for the "pw" keyword (Sevak one-shot plugin).

The query (optional) is a length, e.g. "24". Everything is generated with the
`secrets` module; nothing is stored, logged or sent anywhere.
"""
import json
import math
import re
import secrets
import string
import sys

LETTERS = string.ascii_letters
DIGITS = string.digits
SYMBOLS = "!@#$%^&*-_=+?"
LOOKALIKES = set("0O1lI")


def generate(length, groups):
    """A string of `length` characters with at least one from every group."""
    alphabet = "".join(groups)
    while True:
        text = "".join(secrets.choice(alphabet) for _ in range(length))
        if all(any(c in group for c in text) for group in groups):
            return text


def row(key, label, length, groups):
    text = generate(length, groups)
    bits = int(length * math.log2(len("".join(groups))))
    return {
        "key": key,
        "title": text,
        "subtitle": f"{label}, {length} characters, about {bits} bits. Enter to copy",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": text},
    }


def main():
    query = " ".join(sys.argv[1:]).strip()
    match = re.search(r"\d+", query)
    wanted = max(4, min(128, int(match.group()))) if match else None

    easy_letters = "".join(c for c in LETTERS if c not in LOOKALIKES)
    easy_digits = "".join(c for c in DIGITS if c not in LOOKALIKES)
    pin_length = wanted if wanted and wanted <= 12 else 6
    items = [
        row("strong", "Letters, digits and symbols", wanted or 20, [LETTERS, DIGITS, SYMBOLS]),
        row("letters-digits", "Letters and digits", wanted or 20, [LETTERS, DIGITS]),
        row("easy", "Easy to read (no 0 O 1 l I)", wanted or 16, [easy_letters, easy_digits]),
        row("pin", "Digits only", pin_length, [DIGITS]),
        row("hex", "Hex token", wanted or 32, ["0123456789abcdef"]),
    ]
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
