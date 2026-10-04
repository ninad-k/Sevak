"""Unique identifiers for the "id" keyword (Sevak one-shot plugin).

Standard library only. Everything is generated locally from the operating
system's secure random source; nothing is stored, logged or sent anywhere.
"""
import json
import secrets
import sys
import time
import uuid

CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
NANOID_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"


def uuid7():
    """RFC 9562 UUID version 7: a 48-bit millisecond timestamp, then random bits."""
    millis = int(time.time() * 1000) & ((1 << 48) - 1)
    value = (millis << 80) | (secrets.randbits(12) << 64) | secrets.randbits(62)
    value |= 0x7 << 76  # version 7
    value |= 0x2 << 62  # variant 10
    return str(uuid.UUID(int=value))


def ulid():
    """A 26-character ULID: 48-bit millisecond timestamp, 80 random bits, Crockford base32."""
    value = ((int(time.time() * 1000) & ((1 << 48) - 1)) << 80) | secrets.randbits(80)
    return "".join(CROCKFORD[(value >> shift) & 31] for shift in range(125, -1, -5))


def nanoid(size=21):
    return "".join(secrets.choice(NANOID_ALPHABET) for _ in range(size))


def row(key, label, text):
    return {
        "key": key,
        "title": text,
        "subtitle": f"{label}. Enter to copy",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": text},
    }


def main():
    items = [
        row("uuid4", "UUID v4 (random)", str(uuid.uuid4())),
        row("uuid7", "UUID v7 (sortable by time)", uuid7()),
        row("ulid", "ULID (sortable by time, 26 characters)", ulid()),
        row("nanoid", "NanoID (21 URL-safe characters)", nanoid()),
        row("uuid4-compact", "UUID v4 without dashes", uuid.uuid4().hex),
    ]
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
