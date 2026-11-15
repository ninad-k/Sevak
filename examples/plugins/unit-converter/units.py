"""Unit conversion for the "unit" keyword (Sevak one-shot plugin).

"unit 5 km" lists the value in every unit of its kind; "unit 5 km mi" (or
"in", "to", "as", "=" between the units) converts to one unit first. Also:
cooking by ingredient ("unit 1 cup flour g") and CSS/print sizes at 96 dpi
("unit 2rem px @ 18px", em and rem default to 16px). Standard library only.

Name clashes: "pc" is the parsec, unless the other unit is a typography unit
(px, pt, pica, em, rem); write "pica" to be sure. "pt" is always the point;
spell the volume "pint". Data units are case sensitive (MB bytes, Mb bits).
"""
import json
import math
import re
import sys

# name|alias|...:factor to the base unit. "r<k>" = reciprocal (k / value),
# "T" = temperature (functions in TEMP). Plurals and "sq"/"cu" are automatic.
TABLES = {
    "length": """m|meter|metre:1
km|kilometer|kilometre:1000
cm|centimeter|centimetre:0.01
mm|millimeter|millimetre:0.001
mi|mile:1609.344
ft|foot|feet:0.3048
in|inch|inches:0.0254
yd|yard:0.9144
\u00b5m|um|micron|micrometer|micrometre:1e-6
nm|nanometer|nanometre:1e-9
nmi|nautical mile:1852
fathom:1.8288
furlong:201.168
mil|thou:0.0000254
ly|light year|lightyear:9460730472580800
au|astronomical unit:149597870700
pc|parsec:3.0856775814913673e16""",
    "area": """m2|square meter|square metre:1
km2:1e6
cm2:0.0001
mm2:0.000001
ha|hectare:10000
acre|ac:4046.8564224
ft2:0.09290304
in2:0.00064516
yd2:0.83612736
mi2:2589988.110336""",
    "volume": """L|l|liter|litre:1
mL|milliliter|millilitre|cc|cm3:0.001
cup:0.2365882365
tbsp|tablespoon|tbs:0.01478676478125
tsp|teaspoon:0.00492892159375
fl oz|floz|fluid ounce:0.0295735295625
pint:0.473176473
qt|quart:0.946352946
gal|gallon:3.785411784
m3:1000
ft3:28.316846592
in3:0.016387064
yd3:764.554857984
imp gal|imperial gallon|uk gallon:4.54609
bbl|barrel:158.987294928""",
    "mass": """kg|kilogram|kilo:1
g|gram|gramme:0.001
lb|pound:0.45359237
oz|ounce:0.028349523125
mg|milligram:1e-6
\u00b5g|ug|microgram:1e-9
st|stone:6.35029318
t|tonne|metric ton:1000
ton|short ton|us ton:907.18474
long ton|imperial ton:1016.0469088
ct|carat:0.0002
gr|grain:0.00006479891""",
    "temperature": """\u00b0C|C|celsius|centigrade|degc|deg c|degree celsius|degrees celsius:T
\u00b0F|F|fahrenheit|degf|deg f|degree fahrenheit|degrees fahrenheit:T
K|kelvin|\u00b0K:T""",
    "speed": """m/s|mps|meter/second|metre/second:1
km/h|kmh|kph|kmph|km/hr|kilometer/hour|kilometers/hour|kilometre/hour|kilometres/hour:1/3.6
mph|mi/h|mi/hr|mile/hour|miles/hour:0.44704
kn|kt|knot:1852/3600
ft/s|fps|foot/second|feet/second:0.3048
mach:340.29""",
    "data": """B|byte:8
kB|KB|kilobyte:8000
MB|megabyte:8e6
GB|gigabyte:8e9
TB|terabyte:8e12
PB|petabyte:8e15
KiB|kibibyte:8192
MiB|mebibyte:8388608
GiB|gibibyte:8589934592
TiB|tebibyte:8796093022208
b|bit:1
Kb|kbit|kilobit:1000
Mb|Mbit|megabit:1e6
Gb|Gbit|gigabit:1e9
Tb|Tbit|terabit:1e12""",
    "time": """s|sec|second:1
min|minute:60
h|hr|hour:3600
d|day:86400
wk|week:604800
mo|month:2629800
yr|y|year:31557600
ms|millisecond:0.001
\u00b5s|us|microsecond:1e-6
ns|nanosecond:1e-9
decade:315576000""",
    "pressure": """Pa|pascal:1
kPa|kilopascal:1000
bar:1e5
psi:6894.757293168361
atm|atmosphere:101325
mmHg:133.322387415
hPa|hectopascal:100
MPa|megapascal:1e6
mbar|millibar:100
torr:133.32236842105263
inHg:3386.389""",
    "energy": """J|joule:1
kJ|kilojoule:1000
cal|calorie:4.184
kcal|Cal|kilocalorie|food calorie:4184
Wh|watt hour:3600
kWh|kilowatt hour:3.6e6
MJ|megajoule:1e6
BTU:1055.05585262
eV|electronvolt:1.602176634e-19
ft lbf|ft lb|foot pound:1.3558179483314004""",
    "angle": """deg|\u00b0|degree|\u00ba:1
rad|radian:57.29577951308232
grad|gon|gradian:0.9
turn|rev|revolution|cycle:360
arcmin|arcminute|amin:1/60
arcsec|arcsecond|asec:1/3600""",
    "power": """W|watt:1
kW|kilowatt:1000
hp|horsepower|bhp:745.69987158227022
PS|cv|metric hp|metric horsepower:735.49875
MW|megawatt:1e6
GW|gigawatt:1e9
mW|milliwatt:0.001
BTU/h|btu/hr:0.29307107017222""",
    "frequency": """Hz|hertz:1
kHz|kilohertz:1000
MHz|megahertz:1e6
GHz|gigahertz:1e9
THz|terahertz:1e12
mHz|millihertz:0.001
rpm|r/min:1/60
rad/s|radian/second:0.15915494309189535""",
    "force": """N|newton:1
kN|kilonewton:1000
lbf|pound force|poundforce:4.4482216152605
kgf|kilogram force:9.80665
gf|gram force:0.00980665
ozf|ounce force:0.2780138509537812
dyn|dyne:1e-5""",
    "torque": """Nm|N m|newton meter|newton metre:1
kNm|kN m|kilonewton meter|kilonewton metre:1000
lbf ft|lb ft|pound foot:1.3558179483314004
lbf in|in lbf|in lb|lb in|inch pound:0.1129848290276167
kgf m|kgfm:9.80665
ozf in|in ozf:0.00706155176""",
    "data rate": """Mbps|Mb/s|Mbit/s|megabit/s:1e6
kbps|Kbps|kb/s|Kb/s|kbit/s:1000
Gbps|Gb/s|Gbit/s:1e9
MB/s|MBps|megabyte/s:8e6
kB/s|KB/s|kBps|KBps:8000
GB/s|GBps:8e9
bps|bit/s|b/s:1
B/s|Bps|byte/s:8
Tbps|Tb/s:1e12
TB/s|TBps:8e12
KiB/s:8192
MiB/s:8388608
GiB/s:8589934592""",
    "fuel economy": """L/100km|lp100km:1
km/L|kmpl|kpl:r100
mpg|mpg us|us mpg|mi/gal|miles/gallon|mile/gallon:r378.5411784/1.609344
mpg uk|uk mpg|imp mpg|mpg imp:r454.609/1.609344""",
    "typography": """px|pixel:1
pt|point:96/72
pica|pc:16
em:16
rem:16
in|inch|inches:96
cm:96/2.54
mm:96/25.4""",
}

# Grams per US cup (236.5882365 mL) of each ingredient.
INGREDIENTS = """all-purpose flour|flour|all purpose flour|ap flour|plain flour:125
bread flour:127
sugar|granulated sugar|white sugar|caster sugar:200
brown sugar:220
powdered sugar|icing sugar|confectioners sugar:120
butter:227
rice|white rice:185
oats|rolled oats|oatmeal:90
water:236.6
milk:245
honey:340
oil|olive oil|vegetable oil:218
salt:292
cocoa|cocoa powder:85"""
CUP_ML = 236.5882365
COOK_UNITS = ("g", "oz", "lb", "kg", "cup", "tbsp", "tsp", "fl oz", "mL")

# Celsius is the base: (to Celsius, from Celsius).
TEMP = [
    (lambda v: v, lambda c: c),
    (lambda f: (f - 32) * 5 / 9, lambda c: c * 9 / 5 + 32),
    (lambda k: k - 273.15, lambda c: c + 273.15),
]

EXAMPLES = [
    ("5 km", "Length: every unit at once"),
    ("5 km mi", "One target unit, then related ones"),
    ("72 F C", "Temperature"),
    ("100 hp kW", "Power"),
    ("100 Mbps MB/s", "Data rate (bits vs bytes)"),
    ("30 mpg L/100km", "Fuel economy"),
    ("1 cup flour g", "Cooking: volume and weight by ingredient"),
    ("16px pt", "CSS and print sizes at 96 dpi"),
    ("2rem px @ 18px", "em and rem use the base size after @"),
]

EX, LO, CATS, INGR = {}, {}, {}, []


def norm(text):
    text = text.replace("\u00b2", "2").replace("\u00b3", "3")
    text = re.sub(r"[-_\u00b7]", " ", re.sub(r"[.*^']", "", text))
    return " ".join(re.sub(r"\s*/\s*|\s+per\s+", "/", text).split())


def num(text):
    top, _, bottom = text.partition("/")
    return float(top) / float(bottom) if bottom else float(top)


def build():
    for cat, text in TABLES.items():
        CATS[cat] = []
        for line in text.split("\n"):
            names, _, spec = line.rpartition(":")
            if spec == "T":
                factor = ("t",) + TEMP.pop(0)
            elif spec.startswith("r"):
                factor = ("r", num(spec[1:]))
            else:
                factor = num(spec)
            names = names.split("|")
            unit = [names[0], cat, factor]
            CATS[cat].append(unit)
            for key in {norm(n) for n in names}:
                EX.setdefault(key, []).append(unit)
                LO.setdefault(key.lower(), []).append(unit)
    for line in INGREDIENTS.split("\n"):
        names, _, grams = line.rpartition(":")
        names = names.split("|")
        INGR.extend((norm(n), names[0], float(grams)) for n in names)
    INGR.sort(key=lambda entry: -len(entry[0]))


def lookup(phrase, cats=None):
    p = norm(phrase)
    found = []
    m = re.match(r"(sq|square|cu|cubic) (.+)$", p.lower())
    if m:
        suffix = "2" if m.group(1)[0] == "s" else "3"
        found = [EX[u[0] + suffix][0] for u in lookup(m.group(2))
                 if u[1] == "length" and u[0] + suffix in EX]
    for q in (p, p[:-1] if len(p) > 1 and p.endswith("s") else p):
        if found:
            break
        found = EX.get(q) or LO.get(q.lower()) or []
    return [u for u in found if cats is None or u[1] in cats]


def to_base(u, v):
    f = u[2]
    if isinstance(f, tuple):
        return f[1] / v if f[0] == "r" else f[1](v)
    return v * f


def from_base(u, b):
    f = u[2]
    if isinstance(f, tuple):
        return f[1] / b if f[0] == "r" else f[2](b)
    return b / f


def convert(v, a, b, grams_per_cup=None):
    base = to_base(a, v)
    if a[1] != b[1]:
        # Cooking: litres and kilograms differ by the density in kg per litre.
        ratio = grams_per_cup / CUP_ML
        base = base * ratio if a[1] == "volume" else base / ratio
    return from_base(b, base)


def fmt(x, group=False):
    if x != x or x in (math.inf, -math.inf):
        return "overflow"
    x = float("%.10g" % x)
    if x == 0:
        return "0"
    if 1e-6 <= abs(x) < 1e15:
        s = "%.*f" % (max(0, 9 - int(math.floor(math.log10(abs(x))))), x)
        if "." in s:
            s = s.rstrip("0").rstrip(".")
        head, dot, tail = s.partition(".")
        if group:
            head = re.sub(r"(?<=\d)(?=(\d{3})+$)", ",", head)
        return head + dot + tail
    mantissa, exponent = ("%.9e" % x).split("e")
    return "%se%d" % (mantissa.rstrip("0").rstrip("."), int(exponent))


def row(key, title, subtitle, copy=None):
    return {
        "key": key,
        "title": title,
        "subtitle": subtitle,
        "icon": {"kind": "builtin", "name": "copy"},
        "action": {"type": "copy_text", "text": copy if copy is not None else title},
    }


def conv_row(key, v, a, b, cook):
    r = convert(v, a, b, cook[1] if cook else None)
    title = "%s %s" % (fmt(r, True), b[0])
    source = "%s %s%s" % (fmt(v, True), a[0], " " + cook[0] if cook else "")
    return row(key, title, "%s = %s \u00b7 Enter copies" % (source, title),
               "%s %s" % (fmt(r), b[0]))


def take_number(q):
    m = re.match(r"\s*([+-]?(?:\d[\d,_]*(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)"
                 r"(?:\s*/\s*(\d+(?:\.\d+)?))?", q)
    if not m:
        return None, q
    v = float(m.group(1).replace(",", "").replace("_", ""))
    rest = q[m.end():]
    if m.group(2):
        return v / float(m.group(2)), rest
    mixed = re.match(r"\s+(\d+)\s*/\s*(\d+)(?!\d)", rest)
    if mixed:
        v += math.copysign(float(mixed.group(1)) / float(mixed.group(2)), v)
        rest = rest[mixed.end():]
    return v, rest


def solve(toks, cats):
    diag = None
    for i in range(1, len(toks)):
        a, b = lookup(" ".join(toks[:i]), cats), lookup(" ".join(toks[i:]), cats)
        if a and b:
            for u in a:
                for w in b:
                    if u[1] == w[1] or cats:
                        return ("convert", u, w)
            diag = diag or ("mismatch", a[0], b[0])
        elif a or b:
            diag = diag or ("unknown", " ".join(toks[i:] if a else toks[:i]))
    whole = lookup(" ".join(toks), cats)
    if whole:
        return ("browse", whole[0])
    return diag or ("unknown", " ".join(toks))


def suggest(word):
    w = word.lower()
    for n in range(len(w), 1, -1):
        syms = []
        for key, units in LO.items():
            if key.startswith(w[:n]):
                syms.extend(u[0] for u in units if u[0] not in syms)
        if syms:
            return syms[:4]
    return []


def help_rows():
    rows = [row("hint", "Type a number and a unit",
                "length, mass, temperature, data, time, power, cooking, CSS sizes and more", "unit 5 km")]
    for i, (example, what) in enumerate(EXAMPLES):
        rows.append(row("example-%d" % i, "unit " + example, what + " \u00b7 Enter copies", "unit " + example))
    return rows


def results(query):
    q = query.strip()
    if not q:
        return help_rows()
    size = 16.0
    m = re.search(r"@\s*([\d.]+)\s*(px|pt)?\s*$", q, re.I)
    if m:
        size = float(m.group(1)) * (96 / 72 if (m.group(2) or "").lower() == "pt" else 1)
        q = q[:m.start()].strip()
    for u in CATS["typography"]:
        if u[0] in ("em", "rem"):
            u[2] = size
    value, rest = take_number(q)
    if value is None:
        value = 1.0
    if not math.isfinite(value):
        return [row("error", "That number is too large", "Try a smaller number", "unit 5 km")]
    rest = norm(re.sub(r"->|\u2192|=", " = ", rest))
    cook = None
    for alias, name, grams in INGR:
        found = re.search(r"\b%s\b" % re.escape(alias), rest.lower())
        if found:
            cook = (name, grams)
            rest = re.sub(r"\bof\b", " ", rest[:found.start()] + " " + rest[found.end():])
            break
    toks = [t for t in rest.split() if t.lower() not in ("to", "as", "=", ">")]
    if not toks:
        return help_rows()
    last = len(toks) - 1
    attempts = [toks, [t for i, t in enumerate(toks) if not (0 < i < last and t.lower() == "in")]]
    cats = {"volume", "mass"} if cook else None
    diags, outcome = [], None
    for attempt in attempts:
        outcome = solve(attempt, cats)
        if outcome[0] in ("browse", "convert"):
            break
        diags.append(outcome)
    else:
        outcome = next((d for d in diags if d[0] == "mismatch"), diags[-1])
    kind = outcome[0]
    if kind == "mismatch":
        a, b = outcome[1:]
        return [row("mismatch", "Cannot convert %s (%s) to %s (%s)" % (a[0], a[1], b[0], b[1]),
                    "Pick two units of the same kind, for example unit 5 km mi", "unit 5 km mi")]
    if kind == "unknown":
        word = outcome[1]
        hint = suggest(word.split()[0])
        if cook:
            sub = "With an ingredient use volume (tsp tbsp cup fl oz mL L) or weight (g kg oz lb) units"
        else:
            sub = "Did you mean: " + ", ".join(hint) + "?" if hint else "Try km, kg, F, mph, cup, px, GB or MB/s"
        return [row("unknown", ('Unknown unit "%s"' % word)[:100], sub, "unit 5 km")]
    a = outcome[1]
    if value == 0 and "fuel economy" in (a[1], outcome[-1][1]):
        return [row("zero", "Zero has no fuel economy equivalent",
                    "L/100km and km/L or mpg are reciprocals, so 0 cannot be converted", "unit 30 mpg L/100km")]
    pool = [EX[s][0] for s in COOK_UNITS] if cook else CATS[a[1]]
    if kind == "browse":
        return [conv_row(u[0], value, a, u, cook) for u in pool if u is not a]
    b = outcome[2]
    rows = [conv_row("result", value, a, b, cook)]
    rows += [conv_row(u[0], value, a, u, cook) for u in [u for u in pool if u is not a and u is not b][:4]]
    return rows


def main():
    build()
    try:
        items = results(" ".join(sys.argv[1:]))
    except (ValueError, ZeroDivisionError, OverflowError):
        items = [row("error", "That value cannot be converted", "Check the number and the units", "unit 5 km")]
    sys.stdout.write(json.dumps({"items": items}))


if __name__ == "__main__":
    main()
