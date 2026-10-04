"""Color conversions for the "color" keyword (Sevak one-shot plugin).

Reads a color from the query (hex, rgb(), hsl(), three numbers or a CSS name)
and shows it as HEX, RGB, HSL and HSV with its WCAG contrast against white and
black. Standard library only; nothing is stored or sent anywhere.
"""
import colorsys
import json
import re
import sys

NAMES = {
    "black": "000000", "white": "ffffff", "red": "ff0000", "lime": "00ff00", "blue": "0000ff",
    "yellow": "ffff00", "cyan": "00ffff", "aqua": "00ffff", "magenta": "ff00ff", "fuchsia": "ff00ff",
    "silver": "c0c0c0", "gray": "808080", "grey": "808080", "maroon": "800000", "olive": "808000",
    "green": "008000", "purple": "800080", "teal": "008080", "navy": "000080", "orange": "ffa500",
    "pink": "ffc0cb", "brown": "a52a2a", "gold": "ffd700", "coral": "ff7f50", "tomato": "ff6347",
    "salmon": "fa8072", "crimson": "dc143c", "indigo": "4b0082", "violet": "ee82ee",
    "turquoise": "40e0d0", "khaki": "f0e68c", "orchid": "da70d6", "plum": "dda0dd",
    "tan": "d2b48c", "beige": "f5f5dc", "ivory": "fffff0", "lavender": "e6e6fa",
    "chocolate": "d2691e", "skyblue": "87ceeb", "steelblue": "4682b4", "slategray": "708090",
    "royalblue": "4169e1", "forestgreen": "228b22", "limegreen": "32cd32", "hotpink": "ff69b4",
    "firebrick": "b22222", "sienna": "a0522d", "rebeccapurple": "663399",
}
HEX = re.compile(r"#?([0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{4}|[0-9a-f]{3})")
FUNCTION = re.compile(r"(rgba?|hsla?|hsv)\s*\(?([^)]*)\)?")
NUMBER = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)")


def clamp(value, low, high):
    return max(low, min(high, value))


def numbers(text):
    """The numbers in `text` as (value, is_percent) pairs, or None if anything else is in it."""
    leftover = NUMBER.sub("", text).replace("%", "").replace("deg", "")
    if re.search(r"[^\s,/]", leftover):
        return None
    return [(float(m.group()), text[m.end():m.end() + 1] == "%") for m in NUMBER.finditer(text)]


def parse(text):
    """(r, g, b, alpha) with r, g, b in 0-255 and alpha in 0-1, or None."""
    text = text.strip().lower()
    if text in NAMES:
        text = NAMES[text]
    match = HEX.fullmatch(text)
    if match:
        digits = match.group(1)
        if len(digits) in (3, 4):
            digits = "".join(c * 2 for c in digits)
        values = [int(digits[i:i + 2], 16) for i in range(0, len(digits), 2)]
        alpha = values[3] / 255 if len(values) == 4 else 1.0
        return (*values[:3], round(alpha, 3))

    match = FUNCTION.fullmatch(text)
    if match:
        kind, body = match.groups()
        parts = numbers(body)
        if parts is None or len(parts) not in (3, 4):
            return None
        alpha = 1.0
        if len(parts) == 4:
            alpha, is_percent = parts[3]
            alpha = clamp(alpha / 100 if is_percent else alpha, 0, 1)
        first, second, third = parts[:3]
        if kind.startswith("rgb"):
            rgb = [clamp(v * 2.55 if pct else v, 0, 255) for v, pct in (first, second, third)]
        else:
            hue = (first[0] % 360) / 360
            sat = clamp(second[0], 0, 100) / 100
            third_value = clamp(third[0], 0, 100) / 100
            if kind == "hsv":
                rgb = [c * 255 for c in colorsys.hsv_to_rgb(hue, sat, third_value)]
            else:
                rgb = [c * 255 for c in colorsys.hls_to_rgb(hue, third_value, sat)]
        return (*[round(c) for c in rgb], round(alpha, 3))

    parts = numbers(text)
    if parts is not None and len(parts) == 3:
        return (*[round(clamp(v, 0, 255)) for v, _ in parts], 1.0)
    return None


def luminance(r, g, b):
    def linear(channel):
        c = channel / 255
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)


def contrast(first, second):
    high, low = sorted((luminance(*first), luminance(*second)), reverse=True)
    return (high + 0.05) / (low + 0.05)


def percent(value):
    return f"{round(value * 100)}%"


def formats(r, g, b, alpha):
    hue, lightness, saturation = colorsys.rgb_to_hls(r / 255, g / 255, b / 255)
    _, hsv_saturation, value = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
    h = round(hue * 360) % 360
    opaque = alpha >= 1
    suffix = "" if opaque else f", {alpha:g}"
    hex_text = f"#{r:02x}{g:02x}{b:02x}" + ("" if opaque else f"{round(alpha * 255):02x}")
    return [
        ("hex", "HEX", hex_text),
        ("rgb", "RGB", f"rgb{'' if opaque else 'a'}({r}, {g}, {b}{suffix})"),
        ("hsl", "HSL", f"hsl{'' if opaque else 'a'}({h}, {percent(saturation)}, {percent(lightness)}{suffix})"),
        ("hsv", "HSV", f"hsv({h}, {percent(hsv_saturation)}, {percent(value)})"),
    ]


def row(key, label, text, subtitle=None):
    return {
        "key": key,
        "title": text,
        "subtitle": subtitle or f"{label}. Enter to copy",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": text},
    }


def main():
    query = " ".join(sys.argv[1:]).strip()
    color = parse(query) if query else None
    if color is None:
        title = "Type a color" if not query else "That is not a color I know"
        hint = "for example #ff8800, rgb(255, 136, 0), hsl(32 100% 50%), 255 136 0 or tomato"
        items = [{"key": "hint", "title": title, "subtitle": hint,
                  "icon": {"kind": "builtin", "name": "calculator"},
                  "action": {"type": "copy_text", "text": "#ff8800"}}]
    else:
        r, g, b, alpha = color
        items = [row(key, label, text) for key, label, text in formats(r, g, b, alpha)]
        on_white = contrast((r, g, b), (255, 255, 255))
        on_black = contrast((r, g, b), (0, 0, 0))
        best = "black" if on_black > on_white else "white"
        items.append(row(
            "contrast", "Contrast",
            f"{on_white:.2f}:1 on white, {on_black:.2f}:1 on black",
            f"WCAG contrast of this color (AA text needs 4.5:1); {best} text reads best on it",
        ))
        for name, digits in NAMES.items():
            if digits == f"{r:02x}{g:02x}{b:02x}" and alpha >= 1:
                items.append(row("name", "CSS color name", name))
                break
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
