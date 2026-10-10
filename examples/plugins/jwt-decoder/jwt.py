"""Decode a JSON Web Token for the "pyjwt" keyword (Sevak one-shot plugin).

Paste a token (with or without "Bearer ") and see its header, payload and the
time claims (exp, iat, nbf) in plain language. The token is only decoded: the
signature is NOT verified and nothing is validated, so never treat a decoded
token as proof of anything. Standard library only; nothing is stored or sent.
"""
import base64
import datetime
import json
import re
import sys

TIME_CLAIMS = (("iat", "Issued"), ("nbf", "Not before"), ("exp", "Expires"))
SEGMENT = re.compile(r"[A-Za-z0-9_-]*")


def decode_segment(text):
    padded = text + "=" * (-len(text) % 4)
    return json.loads(base64.urlsafe_b64decode(padded.encode("ascii")).decode("utf-8"))


def parse(query):
    """(header, payload, signature) of a token, or None when it is not one."""
    token = query.strip()
    if token.lower().startswith("bearer "):
        token = token[7:].strip()
    parts = token.strip("\"'").split(".")
    if len(parts) != 3 or not all(SEGMENT.fullmatch(p) for p in parts) or not parts[0] or not parts[1]:
        return None
    try:
        header, payload = decode_segment(parts[0]), decode_segment(parts[1])
    except ValueError:
        return None
    if not isinstance(header, dict) or not isinstance(payload, dict):
        return None
    return header, payload, parts[2]


def when(value, now):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    try:
        moment = datetime.datetime.fromtimestamp(value, datetime.timezone.utc)
    except (OverflowError, OSError, ValueError):
        return None
    seconds = int(value - now)
    span = abs(seconds)
    if span < 90:
        rel = f"{span} seconds"
    elif span < 5400:
        rel = f"{round(span / 60)} minutes"
    elif span < 129600:
        rel = f"{round(span / 3600)} hours"
    else:
        rel = f"{round(span / 86400)} days"
    return moment.strftime("%Y-%m-%d %H:%M:%S UTC"), (f"in {rel}" if seconds >= 0 else f"{rel} ago")


def row(key, title, subtitle, copy=None, text=None):
    item = {
        "key": key,
        "title": title[:200],
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }
    if text is not None:
        item["view"] = "text"
        item["text"] = text
    return item


def pretty(value):
    return json.dumps(value, indent=2)


def results(query, now):
    parsed = parse(query) if query.strip() else None
    if parsed is None:
        title = "Paste a JWT to decode it" if not query.strip() else "That does not look like a JWT"
        return [row("hint", title,
                    "header.payload.signature, three dot-separated parts. Decoded here only, never sent",
                    "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig")]
    header, payload, signature = parsed
    items = [
        row("header", json.dumps(header), "Header. Enter to copy, Ctrl+T for pretty JSON", pretty(header), pretty(header)),
        row("payload", json.dumps(payload), "Payload. Enter to copy, Ctrl+T for pretty JSON", pretty(payload), pretty(payload)),
    ]
    for claim, label in TIME_CLAIMS:
        if claim in payload:
            shown = when(payload[claim], now)
            if shown:
                items.append(row(claim, f"{label}: {shown[0]} ({shown[1]})", f"The {claim} claim. Enter to copy", shown[0]))
    exp = payload.get("exp")
    if isinstance(exp, (int, float)) and not isinstance(exp, bool):
        status = "Expired" if exp < now else "Not expired (by its own exp claim)"
        items.append(row("status", status, "Judged by the clock on this computer; the signature is not checked"))
    for claim in ("iss", "sub", "aud", "jti"):
        if claim in payload:
            value = payload[claim] if isinstance(payload[claim], str) else json.dumps(payload[claim])
            items.append(row(claim, f"{claim}: {value}", "Enter to copy the value", value))
    alg = header.get("alg", "?")
    note = "No signature (alg none)" if not signature else f"Signature present, algorithm {alg}; NOT verified"
    items.append(row("signature", note, "This plugin only decodes. It cannot tell you the token is genuine", signature))
    return items[:50]


def main():
    query = " ".join(sys.argv[1:])
    now = datetime.datetime.now(datetime.timezone.utc).timestamp()
    sys.stdout.write(json.dumps({"items": results(query, now)}))


if __name__ == "__main__":
    main()
