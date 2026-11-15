"""Percentage questions for the "pct" keyword (Sevak one-shot plugin).

"15% of 200", "30 of 120", "30 is what % of 120", "80 to 100", "200 + 15%",
"200 - 15%", "15% off 200" and "30 is 15% of what" each get an answer. A bare
"15%" or "0.15" shows its decimal, fraction and percent forms. Standard library
only; nothing is stored or sent.
"""
import json
import math
import re
import sys

NUM = r"(-?(?:\d+(?:\.\d*)?|\.\d+))"
PCT = r"(?:%|percent)"

EXAMPLES = [
    ("15% of 200", "Percent of a number"),
    ("30 of 120", "What percent one number is of another"),
    ("80 to 100", "Percent change and the difference"),
    ("200 + 15%", "Add a percent (use - to subtract)"),
    ("15% off 200", "Price after a discount, and the amount saved"),
    ("30 is 15% of what", "The whole number from a part"),
    ("0.15", "Bare number: decimal, fraction and percent"),
]


class Problem(Exception):
    """A question with no answer. Its title and subtitle are shown as one row."""


def fmt(x):
    if not math.isfinite(x):
        raise Problem("That number is too large to work with", "Try smaller numbers.")
    x = round(x, 10)
    if x == int(x) and abs(x) < 1e15:
        return str(int(x))
    text = "{:.10g}".format(x)
    if "e" in text:
        if abs(x) >= 1:
            return "{:.0f}".format(x)
        text = "{:.10f}".format(x).rstrip("0").rstrip(".")
    return text


def signed(x):
    text = fmt(x)
    return text if text.startswith("-") or text == "0" else "+" + text


def exact(text):
    sign = -1 if text.startswith("-") else 1
    whole, _, fraction = text.lstrip("-").partition(".")
    return sign * int((whole + fraction) or "0"), 10 ** len(fraction)


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": title if copy is None else copy},
    }


def percent_of(p, n):
    p, n = float(p), float(n)
    return [row("result", fmt(p * n / 100), f"{fmt(p)}% of {fmt(n)}. Enter to copy")]


def part_of_whole(part, whole):
    part, whole = float(part), float(whole)
    if whole == 0:
        raise Problem("Cannot divide by zero", "A percentage needs a whole that is not zero.")
    share = part * 100 / whole
    return [row("result", fmt(share) + "%", f"{fmt(part)} is {fmt(share)}% of {fmt(whole)}. Enter to copy")]


def percent_change(start, end):
    start, end = float(start), float(end)
    if start == 0:
        raise Problem("Cannot divide by zero", "Percent change is not defined from 0. Start from a number that is not zero.")
    change = (end - start) * 100 / abs(start)
    return [
        row("change", signed(change) + "%", f"From {fmt(start)} to {fmt(end)}. Enter to copy", copy=fmt(change) + "%"),
        row("diff", fmt(end - start), f"End minus start: {fmt(end)} minus {fmt(start)}"),
    ]


def add_percent(base, sign, p):
    base, p = float(base), float(p)
    factor = (100 + p) if sign == "+" else (100 - p)
    return [row("result", fmt(base * factor / 100), f"{fmt(base)} {sign} {fmt(p)}%. Enter to copy")]


def percent_off(p, n):
    p, n = float(p), float(n)
    saved = n * p / 100
    return [
        row("result", fmt(n - saved), f"{fmt(p)}% off {fmt(n)}. Enter to copy"),
        row("saved", fmt(saved), f"Amount saved at {fmt(p)}% off"),
    ]


def whole_from_part(part, p):
    part, p = float(part), float(p)
    if p == 0:
        raise Problem("Cannot divide by zero", "No whole has a part when the percentage is 0%.")
    whole = part * 100 / p
    return [row("result", fmt(whole), f"{fmt(part)} is {fmt(p)}% of this number. Enter to copy")]


def bare(text, word):
    value = float(text)
    is_percent = word is not None
    decimal = value / 100 if is_percent else value
    percent = value if is_percent else value * 100
    numerator, denominator = exact(text)
    if is_percent:
        denominator *= 100
    shared = math.gcd(numerator, denominator)
    numerator, denominator = numerator // shared, denominator // shared
    rows = [
        row("decimal", fmt(decimal), f"Decimal form of {text}"),
        row("percent", fmt(percent) + "%", f"Percent form of {text}"),
    ]
    if denominator != 1:
        rows.append(row("fraction", f"{numerator}/{denominator}", "Fraction in lowest terms"))
    return rows


HANDLERS = [
    (re.compile(rf"{NUM}\s*{PCT}\s+of\s+{NUM}"), percent_of),
    (re.compile(rf"{NUM}\s+of\s+{NUM}"), part_of_whole),
    (re.compile(rf"{NUM}\s+is\s+what\s*{PCT}\s+of\s+{NUM}"), part_of_whole),
    (re.compile(rf"{NUM}\s+to\s+{NUM}"), percent_change),
    (re.compile(rf"{NUM}\s*([+-])\s*{NUM}\s*{PCT}"), add_percent),
    (re.compile(rf"{NUM}\s*{PCT}\s+off\s+{NUM}"), percent_off),
    (re.compile(rf"{NUM}\s+is\s+{NUM}\s*{PCT}\s+of\s+(?:what|\?)"), whole_from_part),
    (re.compile(rf"{NUM}\s*(%|percent)?"), bare),
]


def results(query):
    text = " ".join(query.lower().split())
    if not text:
        return [row(f"example-{n}", example, f"{what}. Enter to copy", copy=example)
                for n, (example, what) in enumerate(EXAMPLES, 1)]
    for pattern, handler in HANDLERS:
        match = pattern.fullmatch(text)
        if match:
            try:
                return handler(*match.groups())
            except Problem as problem:
                title, subtitle = problem.args
                return [row("error", title, subtitle)]
            except ValueError:
                return [row("error", "That number is too large to work with", "Try smaller numbers.")]
    return [row("hint", "That is not a percent question I can answer",
                "for example 15% of 200, 30 of 120 or 80 to 100", "15% of 200")]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
