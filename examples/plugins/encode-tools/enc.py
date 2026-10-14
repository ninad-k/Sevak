"""Encode and decode typed text for the "enc" keyword (Sevak one-shot plugin).

"enc hello world" lists Base64, URL-safe Base64, percent-encoding, HTML
entities, hex, ROT13 and a JSON string. When the text also decodes cleanly
(Base64, percent-encoding, HTML entities, hex, a JSON string or backslash-u
escapes) those rows come first. Standard library only; nothing is stored or
sent. Sevak's Universal Actions cover a selection; this covers typed text.
"""
import base64
import json
import re
import string
import sys
import unicodedata

MAX_INPUT = 2000
MAX_TITLE = 200

SAFE_BYTES = frozenset((string.ascii_letters + string.digits + "-_.!~*'()").encode("ascii"))
ROT13 = str.maketrans(
    string.ascii_lowercase + string.ascii_uppercase,
    string.ascii_lowercase[13:] + string.ascii_lowercase[:13]
    + string.ascii_uppercase[13:] + string.ascii_uppercase[:13],
)
NAMED_ENTITIES = {"amp": "&", "lt": "<", "gt": ">", "quot": '"', "apos": "'", "nbsp": "\u00a0"}
ENTITY = re.compile(r"&(#[xX][0-9a-fA-F]{1,8}|#[0-9]{1,8}|[A-Za-z][A-Za-z0-9]{0,31});")
UNICODE_ESCAPE = re.compile(
    r"\\u([dD][89abAB][0-9a-fA-F]{2})\\u([dD][c-fC-F][0-9a-fA-F]{2})"
    r"|\\u([0-9a-fA-F]{4})"
    r"|(\\)"
)


class Unclean(Exception):
    pass


def printable(text):
    return bool(text) and all(c in "\t\n\r" or unicodedata.category(c)[0] != "C" for c in text)


def show(text):
    escapes = {"\t": "\\t", "\n": "\\n", "\r": "\\r"}
    pieces = []
    for char in text:
        if char in escapes:
            pieces.append(escapes[char])
        elif unicodedata.category(char)[0] == "C":
            pieces.append("\\u%04x" % ord(char))
        else:
            pieces.append(char)
    shown = "".join(pieces)
    return shown if shown.strip() else "(blank)"


def decode_utf8(raw):
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return None


def base64_decode(source):
    if not re.fullmatch(r"[A-Za-z0-9+/_-]+={0,2}", source):
        return None
    body = source.rstrip("=")
    if len(body) % 4 == 1 or ("=" in source and len(source) % 4):
        return None
    body = body.replace("-", "+").replace("_", "/")
    try:
        raw = base64.b64decode(body + "=" * (-len(body) % 4), validate=True)
    except ValueError:
        return None
    return decode_utf8(raw)


def percent_decode(source):
    data = source.encode("utf-8")
    out = bytearray()
    i = 0
    while i < len(data):
        if data[i] == 0x25:
            pair = data[i + 1:i + 3]
            if len(pair) != 2 or not all(chr(b) in string.hexdigits for b in pair):
                return None
            out.append(int(pair.decode("ascii"), 16))
            i += 3
        else:
            out.append(data[i])
            i += 1
    return decode_utf8(bytes(out))


def html_decode(source):
    def replace(match):
        name = match.group(1)
        if name[0] == "#":
            hexadecimal = name[1] in "xX"
            number = int(name[2:] if hexadecimal else name[1:], 16 if hexadecimal else 10)
            if not 0 < number <= 0x10FFFF or 0xD800 <= number <= 0xDFFF:
                raise Unclean()
            return chr(number)
        if name in NAMED_ENTITIES:
            return NAMED_ENTITIES[name]
        raise Unclean()
    try:
        return ENTITY.sub(replace, source)
    except Unclean:
        return None


def hex_decode(source):
    if not re.fullmatch(r"(?:[0-9a-fA-F]{2})+", source):
        return None
    return decode_utf8(bytes.fromhex(source))


def json_decode(source):
    if len(source) < 2 or not (source.startswith('"') and source.endswith('"')):
        return None
    try:
        value = json.loads(source)
    except ValueError:
        return None
    return value if isinstance(value, str) else None


def unicode_decode(source):
    def replace(match):
        if match.group(1) is not None:
            high, low = int(match.group(1), 16), int(match.group(2), 16)
            return chr(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
        if match.group(3) is not None:
            number = int(match.group(3), 16)
            if 0xD800 <= number <= 0xDFFF:
                raise Unclean()
            return chr(number)
        raise Unclean()
    try:
        return UNICODE_ESCAPE.sub(replace, source)
    except Unclean:
        return None


DECODERS = (
    ("b64-decoded", "Decoded from Base64 (standard or URL-safe)", base64_decode),
    ("url-decoded", "Decoded from percent-encoding (%XX)", percent_decode),
    ("html-decoded", "Decoded from HTML entities", html_decode),
    ("hex-decoded", "Decoded from hexadecimal UTF-8 bytes", hex_decode),
    ("json-decoded", "Decoded from a JSON string literal", json_decode),
    ("unicode", "Decoded from backslash-u escapes", unicode_decode),
)


def percent_encode(data):
    return "".join(chr(byte) if byte in SAFE_BYTES else "%%%02X" % byte for byte in data)


def html_encode(text):
    return (text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace('"', "&quot;").replace("'", "&#39;"))


def encodings(text, data):
    return [
        ("b64", base64.b64encode(data).decode("ascii"), "Base64 of the UTF-8 bytes"),
        ("b64url", base64.urlsafe_b64encode(data).decode("ascii").rstrip("="), "Base64, URL-safe, no padding"),
        ("url", percent_encode(data), "Percent-encoded like encodeURIComponent"),
        ("html", html_encode(text), "HTML entities for & < > \" and '"),
        ("hex", data.hex(), "Hexadecimal UTF-8 bytes"),
        ("rot13", text.translate(ROT13), "ROT13 of the letters A-Z and a-z"),
        ("json", json.dumps(text, ensure_ascii=False), "JSON string literal, quotes included"),
    ]


def row(key, title, subtitle, copy=None):
    shown = show(title)
    if len(shown) > MAX_TITLE:
        shown = shown[:MAX_TITLE - 3] + "..."
    return {
        "key": key,
        "title": shown,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def results(query):
    if not query.strip():
        return [
            row("help-type", "Type text after enc to encode or decode it",
                "For example: enc hello world. Offline, nothing is stored or sent", "hello world"),
            row("help-decode", "Decoded rows come first when the text decodes cleanly",
                "Base64, %XX, HTML entities, hex, JSON strings, backslash-u escapes", "aGVsbG8gd29ybGQ="),
        ]
    if len(query) > MAX_INPUT:
        return [row("too-long", f"Too long to encode: {len(query)} characters, the limit is {MAX_INPUT}",
                    "Shorten the text and try again. Nothing was encoded")]
    try:
        data = query.encode("utf-8")
    except UnicodeEncodeError:
        return [row("not-text", "That input is not valid UTF-8 text", "Nothing was encoded")]
    source = query.strip()
    items = []
    for key, label, decode in DECODERS:
        decoded = decode(source)
        if decoded is None or decoded == source or not printable(decoded):
            continue
        items.append(row(key, decoded, f"{label}. Enter to copy", decoded))
    for key, value, subtitle in encodings(query, data):
        items.append(row(key, value, f"{subtitle}. Enter to copy", value))
    return items[:50]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
