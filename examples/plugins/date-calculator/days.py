"""Date arithmetic for the "days" keyword (Sevak one-shot plugin).

  days 2026-12-25               days until (or since) a date
  days 2026-01-01 2026-12-31    the distance between two dates
  days +90 / days -30           the date 90 days from today / 30 days ago
  days 2026-03-01 + 45          the date 45 days after another date

Dates are YYYY-MM-DD (or YYYY/MM/DD), "today" and "tomorrow". Standard library
only; nothing is stored or sent anywhere.
"""
import datetime
import json
import re
import sys

WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]
DATE = r"(?:\d{4}[-/]\d{1,2}[-/]\d{1,2}|today|tomorrow|yesterday)"


def parse_date(text, today):
    text = text.strip().lower()
    if text in ("today", "now"):
        return today
    if text == "tomorrow":
        return today + datetime.timedelta(days=1)
    if text == "yesterday":
        return today - datetime.timedelta(days=1)
    match = re.fullmatch(r"(\d{4})[-/](\d{1,2})[-/](\d{1,2})", text)
    if not match:
        return None
    try:
        return datetime.date(*(int(g) for g in match.groups()))
    except ValueError:
        return None


def label(day):
    return f"{WEEKDAYS[day.weekday()]} {day.isoformat()}"


def plural(n, word):
    return f"{n} {word}{'' if n == 1 else 's'}"


def span(first, last):
    """Years, months and days from first to last (first <= last)."""
    months = (last.year - first.year) * 12 + last.month - first.month
    if last.day < first.day:
        months -= 1
    year, month = divmod(first.year * 12 + first.month - 1 + months, 12)
    month += 1
    first_of_month = datetime.date(year, month, 1)
    next_month = datetime.date(year + (month == 12), month % 12 + 1, 1)
    anchor = first_of_month.replace(day=min(first.day, (next_month - first_of_month).days))
    return months // 12, months % 12, (last - anchor).days


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def distance(start, end, today):
    n = (end - start).days
    higher, lower = (end, start) if n >= 0 else (start, end)
    years, months, days = span(lower, higher)
    parts = [plural(v, w) for v, w in ((years, "year"), (months, "month"), (days, "day")) if v]
    breakdown = ", ".join(parts) or "0 days"
    if start == today:
        headline = (f"in {plural(n, 'day')}" if n > 0 else f"{plural(-n, 'day')} ago" if n < 0 else "today")
    else:
        headline = plural(abs(n), "day") if n else "the same day"
    items = [
        row("days", headline, f"{label(start)} to {label(end)}. Enter to copy", str(abs(n))),
        row("breakdown", breakdown, "As years, months and days"),
        row("weeks", f"{plural(abs(n) // 7, 'week')} and {plural(abs(n) % 7, 'day')}", f"{abs(n) / 7:.2f} weeks in all. Enter to copy that", f"{abs(n) / 7:.2f}"),
    ]
    business = sum(1 for i in range(abs(n)) if (min(start, end) + datetime.timedelta(days=i)).weekday() < 5) if abs(n) <= 36525 else None
    if business is not None:
        items.append(row("business", f"{plural(business, 'weekday')} (Monday to Friday)", "Counts the first date, not the last; ignores holidays", str(business)))
    return items


def shifted(base, n):
    try:
        target = base + datetime.timedelta(days=n)
    except OverflowError:
        return [row("range", "That date is out of range", "Years 1 to 9999 are supported", "n/a")]
    week = target.isocalendar()
    return [
        row("date", label(target), f"{plural(abs(n), 'day')} {'after' if n >= 0 else 'before'} {label(base)}. Enter to copy", target.isoformat()),
        row("iso", target.isoformat(), "ISO 8601 date. Enter to copy"),
        row("week", f"Day {target.timetuple().tm_yday} of {target.year}, ISO week {week[1]}", "Day of the year and ISO week number"),
    ]


def results(query, today):
    text = query.strip().lower()
    if not text:
        return [row("hint", "Type a date, two dates, or +90", "days 2026-12-25 | days 2026-01-01 2026-12-31 | days +90 | days 2026-03-01 + 45",
                    "days 2026-12-25"),
                row("today", label(today), "Today. Enter to copy", today.isoformat())]
    relative = re.fullmatch(r"([+-]?)\s*(\d{1,7})\s*(?:d|days?)?", text)
    if relative:
        n = int(relative.group(2)) * (-1 if relative.group(1) == "-" else 1)
        return shifted(today, n)
    plus = re.fullmatch(rf"({DATE})\s*([+-])\s*(\d{{1,7}})\s*(?:d|days?)?", text)
    if plus:
        base = parse_date(plus.group(1), today)
        if base:
            return shifted(base, int(plus.group(3)) * (-1 if plus.group(2) == "-" else 1))
    pair = re.fullmatch(rf"({DATE})(?:\s+(?:to|until|and)\s+|\s+)({DATE})", text)
    if pair:
        first, second = parse_date(pair.group(1), today), parse_date(pair.group(2), today)
        if first and second:
            return distance(first, second, today)
    single = re.fullmatch(DATE, text)
    if single:
        end = parse_date(text, today)
        if end:
            return distance(today, end, today)
    return [row("bad", "I could not read that", "Use YYYY-MM-DD dates, today, tomorrow, or +N / -N days", "days 2026-12-25")]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]), datetime.date.today())}))


if __name__ == "__main__":
    main()
