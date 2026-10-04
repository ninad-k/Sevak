"""Placeholder text for the "lorem" keyword (Sevak one-shot plugin).

The query is an amount and a unit: "3 paragraphs", "40 words", "2 sentences"
(a bare number means words). The text is built from a fixed word list with a
fixed seed, so the same request always gives the same text. Standard library
only; nothing is stored or sent anywhere.
"""
import json
import random
import re
import sys

OPENING = "Lorem ipsum dolor sit amet, consectetur adipiscing elit"
WORDS = (
    "a ac accumsan ad adipiscing aenean aliquam aliquet amet ante aptent arcu at auctor augue bibendum "
    "blandit class commodo condimentum congue consectetur consequat conubia convallis cras cubilia curabitur "
    "curae cursus dapibus diam dictum dictumst dignissim dolor donec dui duis efficitur egestas eget eleifend "
    "elementum elit enim erat eros est et etiam eu euismod facilisi facilisis fames faucibus felis fermentum "
    "feugiat finibus fringilla fusce gravida habitant habitasse hac hendrerit himenaeos iaculis id imperdiet "
    "in inceptos integer interdum ipsum justo lacinia lacus laoreet lectus leo libero ligula litora lobortis "
    "lorem luctus maecenas magna magnis malesuada massa mattis mauris maximus metus mi molestie mollis morbi "
    "nam natoque nascetur nec neque netus nibh nisi nisl non nostra nulla nullam nunc odio orci ornare "
    "parturient pellentesque penatibus per pharetra phasellus placerat platea porta porttitor posuere potenti "
    "praesent pretium primis proin pulvinar purus quam quis quisque rhoncus ridiculus risus rutrum sagittis "
    "sapien scelerisque sed sem semper senectus sit sociosqu sodales sollicitudin suscipit suspendisse taciti "
    "tellus tempor tempus tincidunt torquent tortor tristique turpis ullamcorper ultrices ultricies urna ut "
    "varius vehicula vel velit venenatis vestibulum vitae vivamus viverra volutpat vulputate"
).split()


def sentence(rng, first):
    if first:
        return OPENING + "."
    words = [rng.choice(WORDS) for _ in range(rng.randint(6, 14))]
    if len(words) > 8:
        words[len(words) // 2] += ","
    text = " ".join(words)
    return text[0].upper() + text[1:] + "."


def paragraph(rng, first):
    return " ".join(sentence(rng, first and i == 0) for i in range(rng.randint(4, 7)))


def words(rng, count):
    chosen = OPENING.lower().replace(",", "").split()[:count]
    while len(chosen) < count:
        chosen.append(rng.choice(WORDS))
    text = " ".join(chosen)
    return text[0].upper() + text[1:] + "."


LIMITS = {"paragraph": 20, "sentence": 50, "word": 500}


def build(amount, unit):
    rng = random.Random(2024)  # fixed seed: the same request gives the same text
    if unit == "paragraph":
        return "\n\n".join(paragraph(rng, i == 0) for i in range(amount))
    if unit == "sentence":
        return " ".join(sentence(rng, i == 0) for i in range(amount))
    return words(rng, amount)


def row(amount, unit):
    text = build(amount, unit)
    preview = text.replace("\n", " ")
    if len(preview) > 90:
        preview = preview[:90].rstrip() + "..."
    return {
        "key": f"{amount}-{unit}",
        "title": preview,
        "subtitle": f"{amount} {unit}{'' if amount == 1 else 's'}. Enter to copy, Ctrl+T to read it all",
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": text},
        "view": "text",
        "text": text,
    }


def main():
    query = " ".join(sys.argv[1:]).strip().lower()
    match = re.fullmatch(r"(\d+)\s*([a-z]*)", query)
    unit = {"p": "paragraph", "s": "sentence", "w": "word", "": "word"}.get(match.group(2)[:1]) if match else None
    if match and unit:
        items = [row(max(1, min(int(match.group(1)), LIMITS[unit])), unit)]
    else:
        items = [row(1, "paragraph"), row(3, "paragraph"), row(1, "sentence"),
                 row(5, "sentence"), row(20, "word"), row(50, "word")]
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
