"""Checksums for the "hash" keyword (Sevak one-shot plugin).

The query is some text (hashed as UTF-8) or the full path of an existing file
(its bytes are hashed, read in 1 MiB pieces). Standard library only; nothing is
written, stored or sent anywhere.
"""
import hashlib
import json
import os
import sys
import zlib

CHUNK = 1024 * 1024


def algorithms():
    def make(name):
        def factory():
            try:
                return hashlib.new(name, usedforsecurity=False)
            except TypeError:  # Python before 3.9 has no usedforsecurity
                return hashlib.new(name)

        return factory

    wanted = [("md5", "MD5"), ("sha1", "SHA-1"), ("sha256", "SHA-256"), ("sha512", "SHA-512"),
              ("sha3_256", "SHA3-256"), ("blake2b", "BLAKE2b")]
    found = []
    for name, label in wanted:
        try:
            make(name)()
        except (ValueError, TypeError):  # not available (for example MD5 in a FIPS build)
            continue
        found.append((name, label, make(name)))
    return found


def digests(chunks):
    """(key, label, hex digest) for every algorithm over the bytes in `chunks`."""
    found = [(name, label, factory()) for name, label, factory in algorithms()]
    crc = 0
    for chunk in chunks:
        crc = zlib.crc32(chunk, crc)
        for _, _, state in found:
            state.update(chunk)
    rows = [(name, label, state.hexdigest()) for name, label, state in found]
    rows.append(("crc32", "CRC-32", f"{crc & 0xFFFFFFFF:08x}"))
    return rows


def read_chunks(path):
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(CHUNK)
            if not chunk:
                return
            yield chunk


def as_file(query):
    """The existing file the query names by its full path, or None."""
    text = query.strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "\"'":
        text = text[1:-1]
    if text.startswith("~"):
        text = os.path.expanduser(text)
    return text if os.path.isabs(text) and os.path.isfile(text) else None


def size_text(count):
    for unit in ("bytes", "KB", "MB", "GB"):
        if count < 1024 or unit == "GB":
            return f"{count} {unit}" if unit == "bytes" else f"{count:.1f} {unit}"
        count /= 1024


def main():
    query = " ".join(sys.argv[1:]).strip()
    if not query:
        items = [{"key": "hint", "title": "Type some text, or the full path of a file",
                  "subtitle": "for example: hash hello",
                  "icon": {"kind": "builtin", "name": "calculator"},
                  "action": {"type": "copy_text", "text": "hash hello"}}]
        sys.stdout.write(json.dumps({"items": items}))
        return
    path = as_file(query)
    try:
        if path:
            rows = digests(read_chunks(path))
            source = f"file {os.path.basename(path)} ({size_text(os.path.getsize(path))})"
        else:
            data = query.encode("utf-8")
            rows = digests([data])
            source = f"text ({len(data)} bytes, UTF-8)"
    except OSError as err:
        sys.stdout.write(json.dumps({"items": [{"key": "error", "title": "Could not read the file",
                                                "subtitle": str(err.strerror or err)[:200],
                                                "action": {"type": "copy_text", "text": str(path)}}]}))
        return
    items = [
        {
            "key": key,
            "title": digest,
            "subtitle": f"{label} of the {source}. Enter to copy",
            "icon": {"kind": "builtin", "name": "copy"},
            "action": {"type": "copy_text", "text": digest},
        }
        for key, label, digest in rows
    ]
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
