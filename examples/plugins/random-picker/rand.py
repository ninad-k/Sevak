"""Coins, dice, numbers and picks for the "rand" keyword (Sevak one-shot plugin).

"rand" alone shows a coin flip, a d6 and a number from 1 to 100. The other forms
are "rand coin", "rand 2d6", "rand d20", "rand 3d8+2", "rand 1-1000",
"rand 1 to 1000", "rand 50", "rand -10..10", "rand pick a, b, c" and
"rand shuffle a, b, c". Every random choice uses the `secrets` module; nothing is
stored, logged or sent.
"""
import json
import re
import secrets
import sys

MAX_DICE = 100
MIN_SIDES = 2
MAX_SIDES = 1000
MAX_TITLE = 200


def clip(text):
    return text if len(text) <= MAX_TITLE else text[:MAX_TITLE - 3] + "..."


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": clip(title),
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": title if copy is None else copy},
    }


def hint(title, subtitle, example):
    return row("hint", title, subtitle, example)


def help_rows():
    return [
        row("help-coin", "rand coin", "Heads or tails", "rand coin"),
        row("help-dice", "rand 2d6", "Dice: 2d6, d20, 3d8+2 (up to 100 dice, 2 to 1000 sides)", "rand 2d6"),
        row("help-range", "rand 1-1000", "Whole number: 1-1000, 1 to 1000, or 50 for 1 to 50. Negatives work.", "rand 1-1000"),
        row("help-pick", "rand pick a, b, c", "One item from a comma-separated list (| also works)", "rand pick a, b, c"),
        row("help-shuffle", "rand shuffle a, b, c", "The list in a new order", "rand shuffle a, b, c"),
    ]


def coin_row():
    return row("coin", secrets.choice(["Heads", "Tails"]), "Coin flip. Enter to copy")


def range_row(low, high):
    low, high = sorted((low, high))
    value = str(secrets.randbelow(high - low + 1) + low)
    return row("range", value, f"Whole number from {low} to {high}. Enter to copy", value)


def dice(count_text, sides_text, sign, mod_text):
    count = int(count_text) if count_text else 1
    sides = int(sides_text)
    if not 1 <= count <= MAX_DICE:
        return [hint("Roll from 1 to 100 dice", "Examples: 2d6, d20, 3d8+2", "rand 2d6")]
    if not MIN_SIDES <= sides <= MAX_SIDES:
        return [hint("Dice need 2 to 1000 sides", "Examples: 2d6, d20, 3d8+2", "rand 2d6")]
    expr = f"{count}d{sides}"
    modifier = 0
    if sign:
        modifier = int(mod_text) if sign == "+" else -int(mod_text)
        expr += f"{sign}{mod_text}"
    rolls = [secrets.randbelow(sides) + 1 for _ in range(count)]
    total = str(sum(rolls) + modifier)
    rolled = ", ".join(str(roll) for roll in rolls)
    return [row("dice", total, f"{expr}: rolled {rolled}. Enter to copy", total)]


def split_items(text):
    return [part.strip() for part in re.split(r"[,|]", text) if part.strip()]


def shuffled(items):
    items = list(items)
    for i in range(len(items) - 1, 0, -1):
        j = secrets.randbelow(i + 1)
        items[i], items[j] = items[j], items[i]
    return items


def results(query):
    text = query.strip()
    if not text:
        return [coin_row(), row("d6", str(secrets.randbelow(6) + 1), "Six-sided die. Enter to copy"),
                range_row(1, 100)] + help_rows()

    parts = text.split(None, 1)
    word = parts[0].lower()
    rest = parts[1].strip() if len(parts) > 1 else ""
    if word == "coin":
        return [coin_row()]
    if word == "pick":
        items = split_items(rest)
        if not items:
            return [hint("Add the options after pick", "Example: rand pick tea, coffee, water", "rand pick a, b, c")]
        choice = items[secrets.randbelow(len(items))]
        return [row("pick", choice, f"Picked from {len(items)}. Enter to copy")]
    if word == "shuffle":
        items = split_items(rest)
        if not items:
            return [hint("Add the items after shuffle", "Example: rand shuffle a, b, c", "rand shuffle a, b, c")]
        line = ", ".join(shuffled(items))
        return [row("shuffle", line, f"{len(items)} items in a new order. Enter to copy", line)]

    low = text.lower()
    dice_match = re.fullmatch(r"(\d{0,9})d(\d{1,9})(?:([+-])\s*(\d{1,9}))?", re.sub(r"\s+", "", low))
    if dice_match:
        return dice(*dice_match.groups())
    bounds = re.fullmatch(r"(-?\d{1,15})(?:\s*(?:\.\.|to)\s*|\s*-\s*)(-?\d{1,15})", low)
    if bounds:
        return [range_row(int(bounds.group(1)), int(bounds.group(2)))]
    if re.fullmatch(r"-?\d{1,15}", low):
        top = int(low)
        if top < 1:
            return [hint("Top number must be 1 or more", "Examples: rand 50, rand -10..10", "rand 50")]
        return [range_row(1, top)]
    return [hint("Not a roll, number or list I know", "Examples below. Enter copies the example", "rand 2d6")] + help_rows()


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
