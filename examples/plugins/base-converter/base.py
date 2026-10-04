"""Number-base conversion for the "base" keyword (Sevak one-shot plugin).

"base 255", "base 0xff", "base 0b1010", "base 0o17" or "base ff 16" (a number
and the base it is written in) show binary, octal, decimal and hexadecimal,
plus base 32 and base 36. Standard library only; nothing is stored or sent.
"""
import json
import sys

DIGITS = "0123456789abcdefghijklmnopqrstuvwxyz"
PREFIXES = {"0x": 16, "0b": 2, "0o": 8}
MAX_BITS = 256


def to_base(value, base):
    n = abs(value)
    out = []
    while n:
        n, digit = divmod(n, base)
        out.append(DIGITS[digit])
    return "".join(reversed(out)) or "0"


def group(text, size):
    text = text.rjust(-(-len(text) // size) * size, "0")
    return " ".join(text[i:i + size] for i in range(0, len(text), size))


def parse(query):
    """(value, base) or None. A prefix or a trailing base chooses the base."""
    words = query.replace("_", "").replace(",", "").split()
    if not words or len(words) > 2:
        return None
    text = words[0].lower()
    base = None
    if len(words) == 2:
        if not words[1].isdigit() or not 2 <= int(words[1]) <= 36:
            return None
        base = int(words[1])
    sign = -1 if text.startswith("-") else 1
    text = text.lstrip("+-")
    if text[:2] in PREFIXES:
        if base not in (None, PREFIXES[text[:2]]):
            return None
        base, text = PREFIXES[text[:2]], text[2:]
    if not text or not text.isalnum() or not text.isascii():
        return None
    try:
        value = int(text, base or 10)
    except ValueError:
        return None
    if value.bit_length() > MAX_BITS:
        return None
    return sign * value, base or 10


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def results(query):
    parsed = parse(query) if query.strip() else None
    if parsed is None:
        title = "Type a number" if not query.strip() else "That is not a number I can convert"
        return [row("hint", title, "for example 255, 0xff, 0b1010, 0o17, or ff 16 (a number and its base)", "0xff")]
    value, base = parsed
    sign = "-" if value < 0 else ""
    hex_text = to_base(value, 16)
    bin_text = to_base(value, 2)
    bits = value.bit_length()
    return [
        row("dec", str(value), "Decimal. Enter to copy"),
        row("hex", f"{sign}0x{hex_text}", "Hexadecimal. Enter to copy"),
        row("bin", f"{sign}0b{bin_text}", "Binary. Enter to copy"),
        row("oct", f"{sign}0o{to_base(value, 8)}", "Octal. Enter to copy"),
        row("bin-grouped", sign + group(bin_text, 4), "Binary in groups of four bits"),
        row("hex-grouped", sign + group(hex_text, 2), "Hexadecimal bytes"),
        row("b32", sign + to_base(value, 32), "Base 32 (digits 0-9 a-v)"),
        row("b36", sign + to_base(value, 36), "Base 36 (digits 0-9 a-z)"),
        row("size", f"{max(bits, 1)} bit{'' if bits <= 1 else 's'}, {max(-(-bits // 8), 1)} byte{'' if bits <= 8 else 's'}",
            f"Smallest unsigned size of the number (typed in base {base})"),
    ]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
