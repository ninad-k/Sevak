"""Number spelling and notation for the "num" keyword (Sevak one-shot plugin).

"num 1234" spells the number out in English and gives ordinal, Roman, grouped,
scientific, engineering and SI-prefix forms. "num mcmxciv" reads a Roman
numeral as its decimal value. Standard library only; nothing is stored or sent.
"""
import json
import re
import sys

ONES = ["", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
        "seventeen", "eighteen", "nineteen"]
TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]
DIGITS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
SCALES = ["", "thousand", "million", "billion", "trillion"]
ORDINAL_IRREGULAR = {"one": "first", "two": "second", "three": "third", "five": "fifth",
                     "eight": "eighth", "nine": "ninth", "twelve": "twelfth"}
ROMAN_SYMBOLS = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
                 (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
ROMAN_VALUES = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100, "D": 500, "M": 1000}
ROMAN_RE = re.compile(r"M{0,3}(CM|CD|D?C{0,3})(XC|XL|L?X{0,3})(IX|IV|V?I{0,3})")
NUMBER_RE = re.compile(r"([+-]?)([0-9]*)(?:\.([0-9]*))?(?:[eE]([+-]?[0-9]+))?")
GROUPED_RE = re.compile(r"[+-]?[0-9]{1,3}(?:[,_][0-9]{3})+(?:\.[0-9]+)?")
SI_PREFIX = {24: "Y", 21: "Z", 18: "E", 15: "P", 12: "T", 9: "G", 6: "M", 3: "k", 0: "",
             -3: "m", -6: "u", -9: "n", -12: "p", -15: "f", -18: "a", -21: "z", -24: "y"}
MAX_DIGITS = 15
ZERO = (False, "0", 0)
NOT_A_NUMBER = ("That is not a number I can convert",
                "For example 1234, -42, 3.14, 1,234 or 1e6, or a Roman numeral")
TOO_BIG = ("That number is too big to spell out",
           "Up to 15 digits before the decimal point")
TOO_PRECISE = ("That number has too many decimal places",
               "Up to 15 digits after the decimal point")


class Refused(Exception):
    def __init__(self, title, subtitle):
        super().__init__(title)
        self.title = title
        self.subtitle = subtitle


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def below_thousand(n):
    hundreds, rest = divmod(n, 100)
    parts = []
    if hundreds:
        parts.append(ONES[hundreds] + " hundred")
    if rest < 20:
        if rest:
            parts.append(ONES[rest])
    else:
        tens, ones = divmod(rest, 10)
        parts.append(TENS[tens] + ("-" + ONES[ones] if ones else ""))
    return " ".join(parts)


def cardinal(n):
    if n == 0:
        return "zero"
    groups = []
    scale = 0
    while n:
        n, group = divmod(n, 1000)
        if group:
            name = below_thousand(group)
            groups.append(name + (" " + SCALES[scale] if SCALES[scale] else ""))
        scale += 1
    return " ".join(reversed(groups))


def ordinal(n):
    if n % 100 in (11, 12, 13):
        suffix = "th"
    else:
        suffix = {1: "st", 2: "nd", 3: "rd"}.get(n % 10, "th")
    return f"{n}{suffix}"


def ordinal_words(n):
    stem, _, last = cardinal(n).rpartition(" ")
    head, dash, word = last.rpartition("-")
    if word in ORDINAL_IRREGULAR:
        word = ORDINAL_IRREGULAR[word]
    elif word.endswith("y"):
        word = word[:-1] + "ieth"
    else:
        word += "th"
    return " ".join(part for part in (stem, head + dash + word) if part)


def to_roman(n):
    out = []
    for value, symbol in ROMAN_SYMBOLS:
        count, n = divmod(n, value)
        out.append(symbol * count)
    return "".join(out)


def roman_value(text):
    if not ROMAN_RE.fullmatch(text):
        return None
    total = 0
    for index, symbol in enumerate(text):
        value = ROMAN_VALUES[symbol]
        if index + 1 < len(text) and value < ROMAN_VALUES[text[index + 1]]:
            total -= value
        else:
            total += value
    if 1 <= total <= 3999 and to_roman(total) == text:
        return total
    return None


def from_int(n):
    digits = str(abs(n))
    stripped = digits.rstrip("0")
    if not stripped:
        return ZERO
    return (n < 0, stripped, len(digits) - len(stripped))


def parse_number(text):
    """(negative, digits, exponent) meaning digits x 10**exponent, digits without outer zeros."""
    if GROUPED_RE.fullmatch(text):
        text = text.replace(",", "").replace("_", "")
    match = NUMBER_RE.fullmatch(text)
    if not match or not (match.group(2) or match.group(3)):
        raise Refused(*NOT_A_NUMBER)
    try:
        exponent = int(match.group(4) or "0")
    except ValueError:
        raise Refused(*NOT_A_NUMBER)
    fraction = match.group(3) or ""
    digits = (match.group(2) + fraction).lstrip("0")
    exponent -= len(fraction)
    if not digits:
        return ZERO
    stripped = digits.rstrip("0")
    exponent += len(digits) - len(stripped)
    if len(stripped) + exponent > MAX_DIGITS:
        raise Refused(*TOO_BIG)
    if exponent < -MAX_DIGITS:
        raise Refused(*TOO_PRECISE)
    return (match.group(1) == "-", stripped, exponent)


def plain(num, signed=True):
    negative, digits, exponent = num
    sign = "-" if negative and signed else ""
    if exponent >= 0:
        body = digits + "0" * exponent
    elif len(digits) + exponent > 0:
        point = len(digits) + exponent
        body = digits[:point] + "." + digits[point:]
    else:
        body = "0." + "0" * -(len(digits) + exponent) + digits
    return sign + body


def parts(num):
    whole, _, fraction = plain(num, signed=False).partition(".")
    return whole, fraction


def words(num):
    whole, fraction = parts(num)
    text = cardinal(int(whole))
    if fraction:
        text += " point " + " ".join(DIGITS[int(digit)] for digit in fraction)
    return "minus " + text if num[0] else text


def grouped(num):
    whole, fraction = parts(num)
    text = format(int(whole), ",")
    if fraction:
        text += "." + fraction
    return ("-" if num[0] else "") + text


def split_exponent(num, step):
    _, digits, exponent = num
    if digits == "0":
        return "0", 0
    top = len(digits) - 1 + exponent
    base = top // step * step
    point = top - base + 1
    digits = digits.ljust(point, "0")
    mantissa = digits[:point] + ("." + digits[point:] if digits[point:] else "")
    return mantissa, base


def exponent_text(exponent):
    return ("+" if exponent >= 0 else "-") + str(abs(exponent))


def number_rows(num):
    sign = "-" if num[0] else ""
    whole = int(plain(num)) if num[2] >= 0 else None
    rows = [
        row("decimal", plain(num), "Decimal, plain digits"),
        row("words", words(num), "In English words"),
    ]
    if whole is not None and whole >= 0:
        rows.append(row("ordinal", ordinal(whole), "Ordinal number"))
        rows.append(row("ordinal-words", ordinal_words(whole), "Ordinal in words"))
    if whole is not None and 1 <= whole <= 3999:
        rows.append(row("roman", to_roman(whole), "Roman numeral, 1 to 3999"))
    rows.append(row("grouped", grouped(num), "Digits grouped in threes"))
    mantissa, exponent = split_exponent(num, 1)
    rows.append(row("scientific", sign + mantissa + "e" + exponent_text(exponent), "Scientific notation"))
    mantissa, exponent = split_exponent(num, 3)
    rows.append(row("engineering", sign + mantissa + "e" + exponent_text(exponent),
                    "Engineering notation, exponent a multiple of 3"))
    if exponent in SI_PREFIX:
        prefix = SI_PREFIX[exponent]
        rows.append(row("si", sign + mantissa + (" " + prefix if prefix else ""),
                        "SI prefix: k kilo, m milli, u micro"))
    return rows


def results(query):
    text = query.strip()
    if not text:
        return [
            row("hint", "Type a number", "For example 1234, -42, 3.14, 1,234 or 1e6", "1234"),
            row("hint-roman", "Or a Roman numeral", "For example MCMXCIV, which is 1994", "MCMXCIV"),
        ]
    value = roman_value(text.upper())
    if value is not None:
        return number_rows(from_int(value))
    try:
        return number_rows(parse_number(text))
    except Refused as refused:
        return [row("hint", refused.title, refused.subtitle, "1234")]


def main():
    sys.stdout.write(json.dumps({"items": results(" ".join(sys.argv[1:]))}))


if __name__ == "__main__":
    main()
