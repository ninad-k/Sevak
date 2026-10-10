"""Explain a cron expression for the "cron" keyword (Sevak one-shot plugin).

Takes the usual five fields (minute hour day-of-month month day-of-week) or a
macro such as @daily, says in words when it runs and lists the next few run
times on this computer's clock. Supports *, lists, ranges, steps and the month
and weekday names. Standard library only; nothing is stored or sent.
"""
import datetime
import json
import sys

MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
DAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]
MONTH_NAMES = ["January", "February", "March", "April", "May", "June", "July", "August",
               "September", "October", "November", "December"]
DAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
MACROS = {
    "@yearly": "0 0 1 1 *", "@annually": "0 0 1 1 *", "@monthly": "0 0 1 * *",
    "@weekly": "0 0 * * 0", "@daily": "0 0 * * *", "@midnight": "0 0 * * *", "@hourly": "0 * * * *",
}
# name, lowest, highest, names (value = index + lowest)
FIELDS = [("minute", 0, 59, None), ("hour", 0, 23, None), ("day of month", 1, 31, None),
          ("month", 1, 12, MONTHS), ("day of week", 0, 7, DAYS)]
EXAMPLES = ["*/15 * * * *", "30 9 * * mon-fri", "0 0 1 * *", "@daily"]


def number(text, low, names):
    if names and text.lower() in names:
        return names.index(text.lower()) + (1 if low == 1 else 0)
    if not text.isdigit():
        raise ValueError(text)
    return int(text)


def parse_field(text, low, high, names):
    """The set of allowed values; raises ValueError when the field is invalid."""
    values = set()
    for part in text.split(","):
        step = 1
        if "/" in part:
            part, _, step_text = part.partition("/")
            if not step_text.isdigit() or int(step_text) < 1:
                raise ValueError(text)
            step = int(step_text)
        if part == "*":
            start, end = low, high
        elif "-" in part:
            first, _, last = part.partition("-")
            start, end = number(first, low, names), number(last, low, names)
        else:
            start = number(part, low, names)
            end = high if step > 1 else start
        if start < low or end > high or start > end:
            raise ValueError(text)
        values.update(range(start, end + 1, step))
    if high == 7:  # day of week: 7 is Sunday too
        values = {v % 7 for v in values}
    return values


def parse(expression):
    """(sets, texts) for a 5-field expression, or None."""
    text = expression.strip()
    text = MACROS.get(text.lower(), text)
    parts = text.split()
    if len(parts) != 5:
        return None
    try:
        sets = [parse_field(p, lo, hi, names) for p, (_, lo, hi, names) in zip(parts, FIELDS)]
    except ValueError:
        return None
    return sets, parts


def ordinal(n):
    suffix = "th" if 10 <= n % 100 <= 20 else {1: "st", 2: "nd", 3: "rd"}.get(n % 10, "th")
    return f"{n}{suffix}"


def join(items):
    items = list(items)
    return items[0] if len(items) == 1 else ", ".join(items[:-1]) + " and " + items[-1]


def runs(values):
    """Consecutive runs of a sorted list as (first, last) pairs."""
    out = []
    for v in sorted(values):
        if out and v == out[-1][1] + 1:
            out[-1][1] = v
        else:
            out.append([v, v])
    return out


def describe_time(minutes, hours, minute_text, hour_text):
    if minute_text == "*" and hour_text == "*":
        return "Every minute"
    if minute_text.startswith("*/") and hour_text == "*" and minute_text[2:].isdigit():
        return f"Every {minute_text[2:]} minutes"
    if len(minutes) == 1 and hour_text == "*":
        return f"At minute {min(minutes)} of every hour"
    if len(minutes) == 1 and len(hours) == 1:
        return f"At {min(hours):02d}:{min(minutes):02d}"
    if len(hours) == 1:
        return f"At minutes {join(str(m) for m in sorted(minutes))} past {min(hours):02d}:00"
    if len(minutes) == 1:
        times = [f"{h:02d}:{min(minutes):02d}" for h in sorted(hours)]
        return f"At {join(times)}" if len(times) <= 6 else f"At minute {min(minutes)} of hours {hour_text}"
    return f"At minutes {minute_text} of hours {hour_text}"


def describe_set(values, labels, base):
    parts = []
    for a, b in runs(values):
        first, last = labels(a - base), labels(b - base)
        parts.append(first if a == b else f"{first} to {last}" if b > a + 1 else f"{first} and {last}")
    return join(parts)


def describe(sets, texts):
    minutes, hours, dom, month, dow = sets
    out = describe_time(minutes, hours, texts[0], texts[1])
    dom_any, dow_any = texts[2] == "*", texts[4] == "*"
    days = []
    if not dom_any:
        days.append("on the " + describe_set(dom, lambda i: ordinal(i + 1), 1) + " of the month")
    if not dow_any:
        days.append("on " + describe_set(dow, lambda i: DAY_NAMES[i], 0))
    if len(days) == 2:
        out += " " + days[0] + " and " + days[1] + " (cron runs when either matches)"
    elif days:
        out += " " + days[0]
    if texts[3] != "*":
        out += " in " + describe_set(month, lambda i: MONTH_NAMES[i], 1)
    return out


def next_runs(sets, start, count=5):
    minutes, hours, dom, month, dow = (sorted(s) for s in sets)
    dom_any, dow_any = len(dom) == 31, len(dow) == 7
    found = []
    day = start.replace(hour=0, minute=0, second=0, microsecond=0)
    for _ in range(366 * 8):
        weekday = (day.weekday() + 1) % 7
        if dom_any and dow_any:
            ok = True
        elif dom_any:
            ok = weekday in dow
        elif dow_any:
            ok = day.day in dom
        else:
            ok = day.day in dom or weekday in dow
        if ok and day.month in month:
            for hour in hours:
                for minute in minutes:
                    moment = day.replace(hour=hour, minute=minute)
                    if moment > start:
                        found.append(moment)
                        if len(found) == count:
                            return found
        day += datetime.timedelta(days=1)
    return found


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title[:200],
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def results(query, now):
    if not query.strip():
        return [row(f"example-{i}", e, "Type a cron expression, for example this one. Enter to copy")
                for i, e in enumerate(EXAMPLES)]
    parsed = parse(query)
    if parsed is None:
        return [row("bad", "That is not a valid cron expression",
                    "Five fields: minute hour day-of-month month day-of-week (or @daily, @hourly...)", "*/15 * * * *")]
    sets, texts = parsed
    summary = describe(sets, texts)
    items = [row("explain", summary, "What it does. Enter to copy")]
    for i, moment in enumerate(next_runs(sets, now)):
        label = f"{DAY_NAMES[(moment.weekday() + 1) % 7][:3]} {moment:%Y-%m-%d %H:%M}"
        items.append(row(f"next-{i}", label, "Next run" if i == 0 else f"Run {i + 1} (this computer's clock)"))
    if len(items) == 1:
        items.append(row("never", "No run in the next 8 years", "For example February 30th never happens"))
    return items


def main():
    now = datetime.datetime.now().replace(second=0, microsecond=0)
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]), now)}))


if __name__ == "__main__":
    main()
