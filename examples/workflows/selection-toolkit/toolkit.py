"""Small text tools for the "Selection toolkit" workflow.

Usage: toolkit.py stats | pretty | minify | time      (the text arrives on stdin)

Standard library only, no network, no files: it reads the text Sevak sends on
standard input and prints the answer on standard output.

  stats    word, character and line counts
  pretty   JSON with two-space indentation (a message when it is not JSON)
  minify   JSON on one line (exit code 1 when it is not JSON, so nothing is pasted)
  time     a Unix timestamp as a date, or a date as a Unix timestamp
"""
import json
import re
import sys
from datetime import datetime, timezone


def plural(count, word):
    return f"{count} {word}" if count == 1 else f"{count} {word}s"


def stats(text):
    words = len(text.split())
    lines = len(text.splitlines())
    return " \u00b7 ".join(
        [plural(words, "word"), plural(len(text), "character"), plural(lines, "line")]
    )


def pretty(text):
    try:
        value = json.loads(text)
    except (ValueError, RecursionError) as err:
        return f"Not valid JSON: {err}"
    return json.dumps(value, indent=2, ensure_ascii=False)


def minify(text):
    try:
        value = json.loads(text)
    except (ValueError, RecursionError):
        sys.exit(1)
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def describe(moment):
    utc = moment.astimezone(timezone.utc)
    local = moment.astimezone()
    return [
        f"UTC:      {utc:%Y-%m-%d %H:%M:%S}",
        f"ISO 8601: {utc:%Y-%m-%dT%H:%M:%S}Z",
        f"Local:    {local:%Y-%m-%d %H:%M:%S %z}",
    ]


def stamps(moment):
    seconds = int(moment.timestamp())
    return [f"Unix seconds:      {seconds}", f"Unix milliseconds: {seconds * 1000}"]


def convert_time(text):
    text = text.strip()
    if re.fullmatch(r"-?\d{1,14}", text):
        number = int(text)
        # Up to 11 digits is seconds (to the year 5138); more is milliseconds.
        milliseconds = len(text.lstrip("-")) >= 12
        try:
            moment = datetime.fromtimestamp(number / 1000 if milliseconds else number, tz=timezone.utc)
        except (OverflowError, OSError, ValueError):
            return "That number is outside the range of dates."
        unit = "milliseconds" if milliseconds else "seconds"
        return "\n".join([f"{text} Unix {unit}", ""] + describe(moment))

    candidate = text.replace("/", "-", 2) if re.match(r"\d{4}/\d{2}/\d{2}", text) else text
    if candidate.endswith(("Z", "z")):
        candidate = candidate[:-1] + "+00:00"
    try:
        moment = datetime.fromisoformat(candidate)
    except ValueError:
        return (
            "Select a Unix timestamp (like 1700000000) or a date "
            "(like 2023-11-14 or 2023-11-14T22:13:20Z)."
        )
    if moment.tzinfo is not None:
        return "\n".join([text, ""] + stamps(moment))
    as_utc = moment.replace(tzinfo=timezone.utc)
    as_local = moment.astimezone()  # naive times are taken as local time
    return "\n".join(
        [f"{text} (no time zone given)", "", "As UTC:"]
        + stamps(as_utc)
        + ["", "As your local time:"]
        + stamps(as_local)
    )


MODES = {"stats": stats, "pretty": pretty, "minify": minify, "time": convert_time}


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode not in MODES:
        sys.exit("usage: toolkit.py stats|pretty|minify|time")
    text = sys.stdin.buffer.read().decode("utf-8", "replace").lstrip("\ufeff")
    try:
        answer = MODES[mode](text)
    except (OverflowError, OSError):
        answer = "That date is outside the range this computer can convert."
    sys.stdout.buffer.write(answer.encode("utf-8"))


if __name__ == "__main__":
    main()
