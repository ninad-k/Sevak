"""Try a regular expression for the "rx" keyword (Sevak one-shot plugin).

Type the pattern, then " :: ", then the text: "rx (\\d+)-(\\d+) :: call 555-1234".
Shows whether it matches and lists every match with its groups, using Python's
`re` syntax (inline flags such as (?i) work). The text is limited to 2000
characters; Sevak's process timeout stops patterns that take too long.
Standard library only; nothing is stored or sent.
"""
import json
import re
import sys

SEPARATOR = " :: "
MAX_TEXT = 2000
MAX_SHOWN = 20


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title[:200],
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def hint(title, subtitle="rx pattern :: text, for example rx (\\w+)@(\\w+) :: ada@example"):
    return [row("hint", title, subtitle, "rx (\\d+)-(\\d+) :: call 555-1234")]


def results(query):
    if not query.strip():
        return hint("Type a pattern, then \" :: \", then the text")
    pattern, separator, text = query.partition(SEPARATOR)
    if not separator:
        pattern, separator, text = query.partition("::")
        pattern, text = pattern.rstrip(), text.lstrip()
    if not separator:
        try:
            re.compile(query.strip())
        except re.error as err:
            return hint("Invalid pattern: " + str(err), "Python re syntax. Add \" :: text\" to test it")
        return hint("Valid pattern. Add \" :: \" and some text to test it")
    try:
        compiled = re.compile(pattern.strip())
    except re.error as err:
        return hint("Invalid pattern: " + str(err), "Python re syntax; check brackets and escapes")
    if len(text) > MAX_TEXT:
        return hint("Text is longer than 2000 characters", "Shorten it to test the pattern")
    matches = list(compiled.finditer(text))
    if not matches:
        return [row("none", "No match", "The pattern was valid but found nothing in the text", "No match")]
    items = [row("count", f"{len(matches)} match{'' if len(matches) == 1 else 'es'}",
                 "Enter to copy all matches, one per line", "\n".join(m.group(0) for m in matches[:200]))]
    for i, match in enumerate(matches[:MAX_SHOWN]):
        shown = match.group(0) if match.group(0) else "(empty match)"
        items.append(row(f"m{i}", shown, f"Match {i + 1} at {match.start()}-{match.end()}. Enter to copy", match.group(0)))
        for number, value in enumerate(match.groups(), 1):
            name = next((n for n, g in compiled.groupindex.items() if g == number), None)
            label = f"{number} ({name})" if name else str(number)
            items.append(row(f"m{i}g{number}", "(no value)" if value is None else value,
                             f"Group {label} of match {i + 1}", value or ""))
    return items[:50]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
