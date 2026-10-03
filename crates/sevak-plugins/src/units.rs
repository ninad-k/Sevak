//! Offline unit conversion for the calculator: `10 km in mi`, `72°F to C`,
//! `5 GB in MiB`, `2 h 30 min in min`, `5'11" to cm`.
//!
//! Syntax: `<quantity> (in|to|as|=|→) <unit>`. A quantity is an expression the
//! calculator can evaluate followed by a unit (`(2+3) km`, `1e3 m`), or a run of
//! number-unit pairs of the same category (`2 h 30 min`, `5 ft 11 in`, `5'11"`).
//!
//! # Units
//!
//! Every category has a base unit, and a unit is `base = (value + offset) *
//! factor`; only temperature has a non-zero offset, which is what makes
//! `32 F = 0 C` work. Spellings come in two kinds:
//!
//! - **case-sensitive** (`MB` megabyte, `Mb` megabit, `B` byte, `b` bit,
//!   `kPa`, `MPa`, `Cal`, `L`), tried first, and
//! - **case-insensitive** (`km`, `kilometers`, `Km`), tried second, which also
//!   accepts plurals (`miles`, `inches`).
//!
//! The pragmatic choices, where people mix up the case, are:
//!
//! - `kb`, `mb`, `gb`, `tb`, `pb` (all lowercase) and `KB` mean bytes (SI,
//!   powers of 1000); `Kb`, `Mb`, `Gb`, `Tb`, `Pb` mean bits. The binary
//!   prefixes are always powers of 1024: `KiB`, `MiB`, `GiB`, `TiB`, `PiB`
//!   (bytes) and `Kib`, `Mib`, ... (bits), also in lowercase as bytes.
//! - `oz` is a mass ounce and `fl oz` a fluid ounce; `pt`, `qt`, `gal` and
//!   `cup` are US customary (use `imp pt`, `imp gal`, ... for imperial);
//!   `ton` is the US short ton and `t`/`tonne` the metric one.
//! - `m` is meters and `min` minutes; `mo` is a month (1/12 of a year), `yr` a
//!   Julian year of 365.25 days.
//! - `cal` is the small calorie (4.184 J), `Cal` the food Calorie (1 kcal).
//! - `°`, `deg` and `degrees` alone are angles; with a letter after
//!   (`°F`, `deg C`, `degrees celsius`) they are temperatures.
//!
//! `km/h`, `m/s`, `mi/h`, `ft/min`, ... and `miles per hour` are built from any
//! length unit over any time unit; `mph`, `kph`, `knots` and friends also work.
//! `sq ft`, `square feet`, `ft2`, `ft²`, `cubic cm`, `cm3`, ... are derived from
//! the common length units.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::calculator::{evaluate, format_number};

/// Significant digits kept in a conversion result. Enough to be exact for
/// every practical use, short enough to read, and it removes `0.30000000000000004`.
const RESULT_DIGITS: usize = 10;

/// Relative size below which a result is rounding noise (`32 F` in `C`).
const NOISE_THRESHOLD: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Length,
    Mass,
    Temperature,
    Volume,
    Area,
    Speed,
    Data,
    Time,
    Pressure,
    Energy,
    Angle,
}

/// A unit of measurement: `base = (value + offset) * factor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub category: Category,
    /// Canonical spelling shown in results (`km`, `°C`, `MiB`, `m²`, `km/h`).
    pub symbol: String,
    factor: f64,
    offset: f64,
}

impl Unit {
    fn to_base(&self, value: f64) -> f64 {
        (value + self.offset) * self.factor
    }

    /// `base` expressed in this unit, with cancellation noise snapped to zero
    /// (`32 F` in `C` is `5.7e-14` in floating point, not `0`).
    fn in_unit(&self, base: f64) -> f64 {
        let scaled = base / self.factor;
        let value = scaled - self.offset;
        if value.abs() < NOISE_THRESHOLD * scaled.abs().max(self.offset.abs()) {
            0.0
        } else {
            value
        }
    }
}

/// Converts `value` from one unit to another of the same category.
pub fn convert(value: f64, from: &Unit, to: &Unit) -> Option<f64> {
    if from.category != to.category {
        return None;
    }
    let out = to.in_unit(from.to_base(value));
    out.is_finite().then_some(out)
}

// ---------------------------------------------------------------------------
// The unit table
// ---------------------------------------------------------------------------

struct Table {
    units: Vec<Unit>,
    /// Case-sensitive spellings.
    exact: HashMap<String, usize>,
    /// Lowercased spellings.
    folded: HashMap<String, usize>,
    /// Spellings claimed twice; always empty (a unit test checks it).
    collisions: Vec<String>,
}

impl Table {
    fn add(
        &mut self,
        category: Category,
        symbol: &str,
        factor: f64,
        offset: f64,
        exact: &[&str],
        names: &[&str],
    ) -> usize {
        let index = self.units.len();
        self.units.push(Unit {
            category,
            symbol: symbol.to_owned(),
            factor,
            offset,
        });
        for spelling in exact {
            self.claim(true, spelling.to_string(), index);
        }
        for name in names {
            self.claim(false, name.to_lowercase(), index);
        }
        index
    }

    fn claim(&mut self, exact: bool, spelling: String, index: usize) {
        let map = if exact {
            &mut self.exact
        } else {
            &mut self.folded
        };
        if let Some(&other) = map.get(&spelling) {
            if other != index {
                self.collisions.push(spelling);
            }
        } else {
            map.insert(spelling, index);
        }
    }
}

/// A length that also yields area and volume units (`ft`, `ft²`, `ft³`).
struct Derivable {
    symbol: &'static str,
    factor: f64,
    names: &'static [&'static str],
}

fn build() -> Table {
    use Category::*;
    let mut t = Table {
        units: Vec::new(),
        exact: HashMap::new(),
        folded: HashMap::new(),
        collisions: Vec::new(),
    };

    // Length (m).
    t.add(
        Length,
        "m",
        1.0,
        0.0,
        &["m"],
        &["m", "meter", "meters", "metre", "metres"],
    );
    t.add(
        Length,
        "km",
        1e3,
        0.0,
        &[],
        &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
    );
    t.add(
        Length,
        "dm",
        0.1,
        0.0,
        &[],
        &["dm", "decimeter", "decimeters", "decimetre", "decimetres"],
    );
    t.add(
        Length,
        "cm",
        1e-2,
        0.0,
        &[],
        &[
            "cm",
            "centimeter",
            "centimeters",
            "centimetre",
            "centimetres",
        ],
    );
    t.add(
        Length,
        "mm",
        1e-3,
        0.0,
        &[],
        &[
            "mm",
            "millimeter",
            "millimeters",
            "millimetre",
            "millimetres",
        ],
    );
    t.add(
        Length,
        "µm",
        1e-6,
        0.0,
        &[],
        &[
            "µm",
            "μm",
            "um",
            "micron",
            "microns",
            "micrometer",
            "micrometers",
            "micrometre",
            "micrometres",
        ],
    );
    t.add(
        Length,
        "nm",
        1e-9,
        0.0,
        &[],
        &["nm", "nanometer", "nanometers", "nanometre", "nanometres"],
    );
    t.add(
        Length,
        "in",
        0.0254,
        0.0,
        &["\"", "″", "”"],
        &["in", "inch", "inches"],
    );
    t.add(
        Length,
        "ft",
        0.3048,
        0.0,
        &["'", "′", "’"],
        &["ft", "foot", "feet"],
    );
    t.add(
        Length,
        "yd",
        0.9144,
        0.0,
        &[],
        &["yd", "yds", "yard", "yards"],
    );
    t.add(Length, "mi", 1609.344, 0.0, &[], &["mi", "mile", "miles"]);
    t.add(
        Length,
        "nmi",
        1852.0,
        0.0,
        &[],
        &["nmi", "nautical mile", "nautical miles"],
    );
    t.add(
        Length,
        "ly",
        9.460_730_472_580_8e15,
        0.0,
        &[],
        &["ly", "lightyear", "lightyears", "light year", "light years"],
    );
    t.add(
        Length,
        "au",
        1.495_978_707e11,
        0.0,
        &[],
        &["au", "astronomical unit", "astronomical units"],
    );
    t.add(
        Length,
        "Å",
        1e-10,
        0.0,
        &[],
        &["å", "angstrom", "angstroms", "ångström"],
    );

    // Area (m²) and volume (L) units derived from the common lengths.
    let derivable = [
        Derivable {
            symbol: "m",
            factor: 1.0,
            names: &["m", "meter", "meters", "metre", "metres"],
        },
        Derivable {
            symbol: "km",
            factor: 1e3,
            names: &["km", "kilometer", "kilometers", "kilometre", "kilometres"],
        },
        Derivable {
            symbol: "dm",
            factor: 0.1,
            names: &["dm", "decimeter", "decimeters"],
        },
        Derivable {
            symbol: "cm",
            factor: 1e-2,
            names: &[
                "cm",
                "centimeter",
                "centimeters",
                "centimetre",
                "centimetres",
            ],
        },
        Derivable {
            symbol: "mm",
            factor: 1e-3,
            names: &[
                "mm",
                "millimeter",
                "millimeters",
                "millimetre",
                "millimetres",
            ],
        },
        Derivable {
            symbol: "in",
            factor: 0.0254,
            names: &["in", "inch", "inches"],
        },
        Derivable {
            symbol: "ft",
            factor: 0.3048,
            names: &["ft", "foot", "feet"],
        },
        Derivable {
            symbol: "yd",
            factor: 0.9144,
            names: &["yd", "yds", "yard", "yards"],
        },
        Derivable {
            symbol: "mi",
            factor: 1609.344,
            names: &["mi", "mile", "miles"],
        },
    ];
    for d in &derivable {
        let area: Vec<String> = d
            .names
            .iter()
            .flat_map(|n| {
                [
                    format!("{n}2"),
                    format!("sq {n}"),
                    format!("sq{n}"),
                    format!("square {n}"),
                ]
            })
            .collect();
        let area: Vec<&str> = area.iter().map(String::as_str).collect();
        t.add(
            Area,
            &format!("{}²", d.symbol),
            d.factor * d.factor,
            0.0,
            &[],
            &area,
        );
        let cubic: Vec<String> = d
            .names
            .iter()
            .flat_map(|n| {
                [
                    format!("{n}3"),
                    format!("cu {n}"),
                    format!("cu{n}"),
                    format!("cubic {n}"),
                ]
            })
            .collect();
        let cubic: Vec<&str> = cubic.iter().map(String::as_str).collect();
        // 1 m³ = 1000 L.
        t.add(
            Volume,
            &format!("{}³", d.symbol),
            d.factor * d.factor * d.factor * 1000.0,
            0.0,
            &[],
            &cubic,
        );
    }
    t.add(Area, "ha", 1e4, 0.0, &[], &["ha", "hectare", "hectares"]);
    t.add(
        Area,
        "ac",
        4_046.856_422_4,
        0.0,
        &[],
        &["ac", "acre", "acres"],
    );

    // Mass (kg).
    t.add(
        Mass,
        "kg",
        1.0,
        0.0,
        &[],
        &["kg", "kilogram", "kilograms", "kilo", "kilos"],
    );
    t.add(
        Mass,
        "g",
        1e-3,
        0.0,
        &[],
        &["g", "gram", "grams", "gramme", "grammes"],
    );
    t.add(
        Mass,
        "mg",
        1e-6,
        0.0,
        &[],
        &["mg", "milligram", "milligrams"],
    );
    t.add(
        Mass,
        "µg",
        1e-9,
        0.0,
        &[],
        &["µg", "μg", "ug", "mcg", "microgram", "micrograms"],
    );
    t.add(
        Mass,
        "t",
        1e3,
        0.0,
        &[],
        &["t", "tonne", "tonnes", "metric ton", "metric tons"],
    );
    t.add(
        Mass,
        "lb",
        0.453_592_37,
        0.0,
        &[],
        &["lb", "lbs", "pound", "pounds"],
    );
    t.add(
        Mass,
        "oz",
        0.028_349_523_125,
        0.0,
        &[],
        &["oz", "ounce", "ounces"],
    );
    t.add(
        Mass,
        "st",
        6.350_293_18,
        0.0,
        &[],
        &["st", "stone", "stones"],
    );
    t.add(
        Mass,
        "ton",
        907.184_74,
        0.0,
        &[],
        &[
            "ton",
            "tons",
            "short ton",
            "short tons",
            "us ton",
            "us tons",
        ],
    );
    t.add(
        Mass,
        "long ton",
        1_016.046_908_8,
        0.0,
        &[],
        &[
            "long ton",
            "long tons",
            "uk ton",
            "uk tons",
            "imperial ton",
            "imperial tons",
        ],
    );
    t.add(
        Mass,
        "gr",
        6.479_891e-5,
        0.0,
        &[],
        &["gr", "grain", "grains"],
    );
    t.add(Mass, "ct", 2e-4, 0.0, &[], &["ct", "carat", "carats"]);

    // Temperature (K): base = (value + offset) * factor.
    t.add(
        Temperature,
        "°C",
        1.0,
        273.15,
        &[],
        &["c", "celsius", "centigrade", "℃"],
    );
    t.add(
        Temperature,
        "°F",
        5.0 / 9.0,
        459.67,
        &[],
        &["f", "fahrenheit", "℉"],
    );
    t.add(Temperature, "K", 1.0, 0.0, &[], &["k", "kelvin", "kelvins"]);
    t.add(
        Temperature,
        "°R",
        5.0 / 9.0,
        0.0,
        &[],
        &["°r", "ra", "rankine"],
    );

    // Volume (L).
    t.add(
        Volume,
        "L",
        1.0,
        0.0,
        &["L"],
        &["l", "liter", "liters", "litre", "litres"],
    );
    t.add(
        Volume,
        "mL",
        1e-3,
        0.0,
        &["mL"],
        &[
            "ml",
            "milliliter",
            "milliliters",
            "millilitre",
            "millilitres",
            "cc",
        ],
    );
    t.add(
        Volume,
        "cL",
        1e-2,
        0.0,
        &["cL"],
        &[
            "cl",
            "centiliter",
            "centiliters",
            "centilitre",
            "centilitres",
        ],
    );
    t.add(
        Volume,
        "dL",
        0.1,
        0.0,
        &["dL"],
        &["dl", "deciliter", "deciliters", "decilitre", "decilitres"],
    );
    t.add(
        Volume,
        "kL",
        1e3,
        0.0,
        &["kL"],
        &["kl", "kiloliter", "kiloliters", "kilolitre", "kilolitres"],
    );
    t.add(
        Volume,
        "µL",
        1e-6,
        0.0,
        &[],
        &["µl", "μl", "ul", "microliter", "microliters"],
    );
    t.add(
        Volume,
        "tsp",
        0.004_928_921_593_75,
        0.0,
        &[],
        &["tsp", "tsps", "teaspoon", "teaspoons"],
    );
    t.add(
        Volume,
        "tbsp",
        0.014_786_764_781_25,
        0.0,
        &[],
        &["tbsp", "tbsps", "tbs", "tablespoon", "tablespoons"],
    );
    t.add(
        Volume,
        "fl oz",
        0.029_573_529_562_5,
        0.0,
        &[],
        &["fl oz", "floz", "fluid ounce", "fluid ounces", "us fl oz"],
    );
    t.add(Volume, "cup", 0.236_588_236_5, 0.0, &[], &["cup", "cups"]);
    t.add(
        Volume,
        "pt",
        0.473_176_473,
        0.0,
        &[],
        &["pt", "pint", "pints", "us pt", "us pint", "us pints"],
    );
    t.add(
        Volume,
        "qt",
        0.946_352_946,
        0.0,
        &[],
        &["qt", "quart", "quarts", "us qt", "us quart", "us quarts"],
    );
    t.add(
        Volume,
        "gal",
        3.785_411_784,
        0.0,
        &[],
        &[
            "gal",
            "gallon",
            "gallons",
            "us gal",
            "us gallon",
            "us gallons",
        ],
    );
    t.add(
        Volume,
        "imp fl oz",
        0.028_413_062_5,
        0.0,
        &[],
        &[
            "imp fl oz",
            "uk fl oz",
            "imperial fluid ounce",
            "imperial fluid ounces",
        ],
    );
    t.add(
        Volume,
        "imp pt",
        0.568_261_25,
        0.0,
        &[],
        &[
            "imp pt",
            "uk pt",
            "imp pint",
            "imp pints",
            "uk pint",
            "uk pints",
            "imperial pint",
            "imperial pints",
        ],
    );
    t.add(
        Volume,
        "imp qt",
        1.136_522_5,
        0.0,
        &[],
        &[
            "imp qt",
            "uk qt",
            "imp quart",
            "imp quarts",
            "uk quart",
            "uk quarts",
            "imperial quart",
            "imperial quarts",
        ],
    );
    t.add(
        Volume,
        "imp gal",
        4.546_09,
        0.0,
        &[],
        &[
            "imp gal",
            "uk gal",
            "imp gallon",
            "imp gallons",
            "uk gallon",
            "uk gallons",
            "imperial gallon",
            "imperial gallons",
        ],
    );

    // Speed (m/s); `X/Y` and `X per Y` are built on the fly in `lookup`.
    t.add(Speed, "mph", 0.447_04, 0.0, &[], &["mph"]);
    t.add(Speed, "km/h", 1.0 / 3.6, 0.0, &[], &["kph", "kmh", "kmph"]);
    t.add(Speed, "m/s", 1.0, 0.0, &[], &["mps"]);
    t.add(Speed, "ft/s", 0.3048, 0.0, &[], &["fps"]);
    t.add(
        Speed,
        "kn",
        1852.0 / 3600.0,
        0.0,
        &[],
        &["kn", "kt", "kts", "knot", "knots"],
    );

    // Data (byte): SI prefixes are powers of 1000, IEC ones powers of 1024.
    t.add(Data, "B", 1.0, 0.0, &["B"], &["byte", "bytes"]);
    t.add(Data, "bit", 0.125, 0.0, &["b"], &["bit", "bits"]);
    let si = [
        ("k", "kilo", 1e3, &["kB", "KB"][..], &["Kb"][..]),
        ("M", "mega", 1e6, &["MB"][..], &["Mb"][..]),
        ("G", "giga", 1e9, &["GB"][..], &["Gb"][..]),
        ("T", "tera", 1e12, &["TB"][..], &["Tb"][..]),
        ("P", "peta", 1e15, &["PB"][..], &["Pb"][..]),
    ];
    for (prefix, word, power, byte_exact, bit_exact) in si {
        let lower = prefix.to_lowercase();
        t.add(
            Data,
            &format!("{prefix}B"),
            power,
            0.0,
            byte_exact,
            &[
                &format!("{lower}b"),
                &format!("{word}byte"),
                &format!("{word}bytes"),
            ]
            .map(String::as_str),
        );
        t.add(
            Data,
            &format!("{prefix}bit"),
            power / 8.0,
            0.0,
            bit_exact,
            &[
                &format!("{lower}bit"),
                &format!("{lower}bits"),
                &format!("{word}bit"),
                &format!("{word}bits"),
            ]
            .map(String::as_str),
        );
    }
    let iec = [
        ("Ki", "kibi", 1024.0_f64),
        ("Mi", "mebi", 1024.0_f64.powi(2)),
        ("Gi", "gibi", 1024.0_f64.powi(3)),
        ("Ti", "tebi", 1024.0_f64.powi(4)),
        ("Pi", "pebi", 1024.0_f64.powi(5)),
    ];
    for (prefix, word, power) in iec {
        let lower = prefix.to_lowercase();
        t.add(
            Data,
            &format!("{prefix}B"),
            power,
            0.0,
            &[format!("{prefix}B").as_str()],
            &[
                &format!("{lower}b"),
                &format!("{word}byte"),
                &format!("{word}bytes"),
            ]
            .map(String::as_str),
        );
        t.add(
            Data,
            &format!("{prefix}bit"),
            power / 8.0,
            0.0,
            &[format!("{prefix}b").as_str()],
            &[
                &format!("{lower}bit"),
                &format!("{lower}bits"),
                &format!("{word}bit"),
                &format!("{word}bits"),
            ]
            .map(String::as_str),
        );
    }

    // Time (s).
    t.add(
        Time,
        "ns",
        1e-9,
        0.0,
        &[],
        &["ns", "nanosecond", "nanoseconds"],
    );
    t.add(
        Time,
        "µs",
        1e-6,
        0.0,
        &[],
        &["µs", "μs", "us", "microsecond", "microseconds"],
    );
    t.add(
        Time,
        "ms",
        1e-3,
        0.0,
        &[],
        &["ms", "msec", "msecs", "millisecond", "milliseconds"],
    );
    t.add(
        Time,
        "s",
        1.0,
        0.0,
        &[],
        &["s", "sec", "secs", "second", "seconds"],
    );
    t.add(
        Time,
        "min",
        60.0,
        0.0,
        &[],
        &["min", "mins", "minute", "minutes"],
    );
    t.add(
        Time,
        "h",
        3600.0,
        0.0,
        &[],
        &["h", "hr", "hrs", "hour", "hours"],
    );
    t.add(Time, "d", 86_400.0, 0.0, &[], &["d", "day", "days"]);
    t.add(
        Time,
        "wk",
        604_800.0,
        0.0,
        &[],
        &["wk", "wks", "week", "weeks"],
    );
    t.add(
        Time,
        "mo",
        2_629_800.0,
        0.0,
        &[],
        &["mo", "mos", "month", "months"],
    );
    t.add(
        Time,
        "yr",
        31_557_600.0,
        0.0,
        &[],
        &["yr", "yrs", "y", "year", "years"],
    );
    t.add(
        Time,
        "decade",
        315_576_000.0,
        0.0,
        &[],
        &["decade", "decades"],
    );
    t.add(
        Time,
        "century",
        3_155_760_000.0,
        0.0,
        &[],
        &["century", "centuries"],
    );

    // Pressure (Pa).
    t.add(Pressure, "Pa", 1.0, 0.0, &[], &["pa", "pascal", "pascals"]);
    t.add(
        Pressure,
        "hPa",
        100.0,
        0.0,
        &[],
        &["hpa", "hectopascal", "hectopascals"],
    );
    t.add(
        Pressure,
        "kPa",
        1e3,
        0.0,
        &[],
        &["kpa", "kilopascal", "kilopascals"],
    );
    t.add(
        Pressure,
        "MPa",
        1e6,
        0.0,
        &["MPa"],
        &["mpa", "megapascal", "megapascals"],
    );
    t.add(
        Pressure,
        "GPa",
        1e9,
        0.0,
        &[],
        &["gpa", "gigapascal", "gigapascals"],
    );
    t.add(Pressure, "bar", 1e5, 0.0, &[], &["bar", "bars"]);
    t.add(
        Pressure,
        "mbar",
        100.0,
        0.0,
        &[],
        &["mbar", "millibar", "millibars"],
    );
    t.add(
        Pressure,
        "atm",
        101_325.0,
        0.0,
        &[],
        &["atm", "atmosphere", "atmospheres"],
    );
    t.add(Pressure, "psi", 6_894.757_293_168, 0.0, &[], &["psi"]);
    t.add(Pressure, "ksi", 6_894_757.293_168, 0.0, &[], &["ksi"]);
    t.add(Pressure, "torr", 101_325.0 / 760.0, 0.0, &[], &["torr"]);
    t.add(Pressure, "mmHg", 133.322_387_415, 0.0, &[], &["mmhg"]);
    t.add(Pressure, "inHg", 3_386.388_640_341, 0.0, &[], &["inhg"]);

    // Energy (J).
    t.add(Energy, "J", 1.0, 0.0, &[], &["j", "joule", "joules"]);
    t.add(
        Energy,
        "kJ",
        1e3,
        0.0,
        &[],
        &["kj", "kilojoule", "kilojoules"],
    );
    t.add(
        Energy,
        "MJ",
        1e6,
        0.0,
        &["MJ"],
        &["megajoule", "megajoules"],
    );
    t.add(
        Energy,
        "cal",
        4.184,
        0.0,
        &[],
        &["cal", "calorie", "calories"],
    );
    t.add(
        Energy,
        "kcal",
        4184.0,
        0.0,
        &["Cal"],
        &["kcal", "kilocalorie", "kilocalories"],
    );
    t.add(
        Energy,
        "Wh",
        3600.0,
        0.0,
        &[],
        &["wh", "watt hour", "watt hours"],
    );
    t.add(
        Energy,
        "kWh",
        3.6e6,
        0.0,
        &[],
        &["kwh", "kilowatt hour", "kilowatt hours"],
    );
    t.add(
        Energy,
        "MWh",
        3.6e9,
        0.0,
        &["MWh"],
        &["megawatt hour", "megawatt hours"],
    );
    t.add(Energy, "BTU", 1_055.055_852_62, 0.0, &[], &["btu", "btus"]);
    t.add(
        Energy,
        "eV",
        1.602_176_634e-19,
        0.0,
        &[],
        &["ev", "electronvolt", "electronvolts"],
    );

    // Angle (rad).
    t.add(Angle, "rad", 1.0, 0.0, &[], &["rad", "radian", "radians"]);
    t.add(
        Angle,
        "°",
        std::f64::consts::PI / 180.0,
        0.0,
        &["°"],
        &["deg", "degs", "degree", "degrees"],
    );
    t.add(
        Angle,
        "grad",
        std::f64::consts::PI / 200.0,
        0.0,
        &[],
        &["grad", "grads", "gradian", "gradians", "gon", "gons"],
    );
    t.add(
        Angle,
        "turn",
        std::f64::consts::TAU,
        0.0,
        &[],
        &["turn", "turns", "rev", "revs", "revolution", "revolutions"],
    );
    t.add(
        Angle,
        "arcmin",
        std::f64::consts::PI / 10_800.0,
        0.0,
        &[],
        &["arcmin", "arcmins", "arcminute", "arcminutes"],
    );
    t.add(
        Angle,
        "arcsec",
        std::f64::consts::PI / 648_000.0,
        0.0,
        &[],
        &["arcsec", "arcsecs", "arcsecond", "arcseconds"],
    );

    t
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(build)
}

// ---------------------------------------------------------------------------
// Looking units up
// ---------------------------------------------------------------------------

/// Canonical form of a unit spelling: single spaces, no dots or carets,
/// `²`/`³` as digits, `per` as `/`, hyphens as spaces.
fn normalize_unit_text(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.trim().chars() {
        match c {
            '²' => s.push('2'),
            '³' => s.push('3'),
            'º' => s.push('°'),
            '^' | '.' => {}
            '-' | '_' => s.push(' '),
            c => s.push(c),
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.replace(" per ", "/")
        .replace(" /", "/")
        .replace("/ ", "/")
}

fn from_table(index: usize) -> Unit {
    table().units[index].clone()
}

/// Looks `text` up in the table: exact spelling, then case-insensitively, then
/// with a plural `s`/`es` removed.
fn lookup_plain(text: &str) -> Option<Unit> {
    let t = table();
    if let Some(&i) = t.exact.get(text) {
        return Some(from_table(i));
    }
    let lower = text.to_lowercase();
    if let Some(&i) = t.folded.get(&lower) {
        return Some(from_table(i));
    }
    for suffix in ["s", "es"] {
        if let Some(stem) = lower.strip_suffix(suffix) {
            if stem.chars().count() >= 2 {
                if let Some(&i) = t.folded.get(stem) {
                    return Some(from_table(i));
                }
            }
        }
    }
    None
}

/// Finds the unit spelled `text`, if any (`km`, `Miles`, `°F`, `km/h`, `sq ft`).
pub fn lookup(text: &str) -> Option<Unit> {
    let text = normalize_unit_text(text);
    if text.is_empty() {
        return None;
    }
    if let Some(unit) = lookup_plain(&text) {
        return Some(unit);
    }
    // `°F`, `deg C`, `degrees celsius`: a degree sign or word before a
    // temperature scale.
    let lower = text.to_lowercase();
    for prefix in ["°", "degrees", "degree", "deg"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if let Some(unit) = lookup_plain(rest.trim()) {
                if unit.category == Category::Temperature {
                    return Some(unit);
                }
            }
        }
    }
    // `km/h`, `ft/min`: any length over any time.
    if let Some((distance, time)) = text.split_once('/') {
        let (distance, time) = (lookup_plain(distance.trim())?, lookup_plain(time.trim())?);
        if distance.category == Category::Length && time.category == Category::Time {
            return Some(Unit {
                category: Category::Speed,
                symbol: format!("{}/{}", distance.symbol, time.symbol),
                factor: distance.factor / time.factor,
                offset: 0.0,
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Parsing "<quantity> in <unit>"
// ---------------------------------------------------------------------------

/// Splits `input` at each ` in `, ` to `, ` as `, `=` or `→`, last first. Both
/// sides are trimmed and non-empty. Several candidates exist because `in` is
/// also inches (`12 in in cm`); the caller takes the first that parses.
pub(crate) fn split_conversion(input: &str) -> Vec<(&str, &str)> {
    let mut candidates = Vec::new();
    for (i, c) in input.char_indices() {
        let width = if c == '=' || c == '→' {
            Some(c.len_utf8())
        } else if c.is_whitespace() {
            let rest = &input[i + c.len_utf8()..];
            ["in", "to", "as"].into_iter().find_map(|word| {
                let head = rest.get(..word.len())?;
                let after = rest[word.len()..].chars().next()?;
                (head.eq_ignore_ascii_case(word) && after.is_whitespace())
                    .then_some(c.len_utf8() + word.len())
            })
        } else {
            None
        };
        if let Some(width) = width {
            let (left, right) = (input[..i].trim(), input[i + width..].trim());
            if !left.is_empty() && !right.is_empty() {
                candidates.push((left, right));
            }
        }
    }
    candidates.reverse();
    candidates
}

/// Every way to read `text` as `<expression> <unit text>`: the expression must
/// evaluate, and the unit text starts right after it (`72°F` -> `72`, `°F`).
pub(crate) fn split_quantity(text: &str) -> Vec<(f64, &str)> {
    let mut out = Vec::new();
    for (i, c) in text.char_indices() {
        if !(c.is_ascii_digit() || matches!(c, '.' | ')' | '!')) {
            continue;
        }
        let split = i + c.len_utf8();
        let suffix = text[split..].trim_start();
        let Some(first) = suffix.chars().next() else {
            continue;
        };
        // The unit starts where the expression cannot continue.
        if first.is_ascii_digit() || "+-*/%^×÷(),=!._".contains(first) {
            continue;
        }
        if let Ok(value) = evaluate(&text[..split]) {
            out.push((value, suffix.trim_end()));
        }
    }
    out
}

/// A measured amount in base units.
struct Quantity {
    base: f64,
    category: Category,
}

fn parse_quantity(text: &str) -> Option<Quantity> {
    for (value, unit_text) in split_quantity(text) {
        if let Some(unit) = lookup(unit_text) {
            return Some(Quantity {
                base: unit.to_base(value),
                category: unit.category,
            });
        }
    }
    parse_compound(text)
}

/// `2 h 30 min`, `5 ft 11 in`, `5'11"`: two or more `number unit` pairs of one
/// category (not temperature), summed.
fn parse_compound(text: &str) -> Option<Quantity> {
    let mut rest = text.trim();
    let negative = rest.starts_with('-');
    if negative || rest.starts_with('+') {
        rest = rest[1..].trim_start();
    }
    let mut total = 0.0;
    let mut category = None;
    let mut parts = 0;
    while !rest.is_empty() {
        let digits_end = rest
            .char_indices()
            .find(|&(_, c)| !(c.is_ascii_digit() || matches!(c, '_' | '.')))
            .map_or(rest.len(), |(i, _)| i);
        let number: f64 = rest[..digits_end].replace('_', "").parse().ok()?;
        let after = rest[digits_end..].trim_start();
        let unit_end = after
            .char_indices()
            .find(|&(i, c)| {
                c.is_ascii_digit()
                    || (c == '.' && after[i + 1..].starts_with(|d: char| d.is_ascii_digit()))
            })
            .map_or(after.len(), |(i, _)| i);
        let unit = lookup(after[..unit_end].trim())?;
        if unit.offset != 0.0 || *category.get_or_insert(unit.category) != unit.category {
            return None;
        }
        total += number * unit.factor;
        parts += 1;
        rest = after[unit_end..].trim_start();
    }
    let category = category.filter(|_| parts >= 2)?;
    Some(Quantity {
        base: if negative { -total } else { total },
        category,
    })
}

/// A successful conversion, ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    /// The query as typed, normalized for the subtitle (`10 km → mi`).
    pub input: String,
    /// The result with its unit (`6.213711922 mi`); also what Enter copies.
    pub text: String,
}

/// Rounds to `digits` significant digits, which removes representation noise
/// (`0.1 + 0.2`) without losing anything a person would read.
pub(crate) fn round_significant(value: f64, digits: usize) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    format!("{:.*e}", digits.saturating_sub(1), value)
        .parse()
        .unwrap_or(value)
}

/// Formats `value` in `unit`: `37.77777778 °C`, `90°`.
fn render(value: f64, unit: &Unit) -> String {
    let number = format_number(round_significant(value, RESULT_DIGITS));
    if unit.symbol == "°" {
        format!("{number}°")
    } else {
        format!("{number} {}", unit.symbol)
    }
}

/// Answers `<quantity> in <unit>` queries; `None` for anything else.
pub fn answer(input: &str) -> Option<Conversion> {
    for (left, right) in split_conversion(input) {
        let (Some(to), Some(quantity)) = (lookup(right), parse_quantity(left)) else {
            continue;
        };
        if quantity.category != to.category {
            continue;
        }
        let value = to.in_unit(quantity.base);
        if !value.is_finite() {
            continue;
        }
        return Some(Conversion {
            input: format!("{left} → {right}"),
            text: render(value, &to),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(input: &str) -> String {
        answer(input)
            .unwrap_or_else(|| panic!("`{input}` should convert"))
            .text
    }

    fn unit(text: &str) -> Unit {
        lookup(text).unwrap_or_else(|| panic!("`{text}` should be a unit"))
    }

    fn close(input: &str, expected: f64) {
        let c = conv(input);
        let number: f64 = c
            .split(' ')
            .next()
            .unwrap()
            .trim_end_matches('°')
            .parse()
            .unwrap_or_else(|_| panic!("`{c}` has no number"));
        assert!(
            (number - expected).abs() <= 1e-6 * expected.abs().max(1.0),
            "`{input}` = {c}, expected {expected}"
        );
    }

    #[test]
    fn table_has_no_ambiguous_spellings() {
        assert_eq!(table().collisions, Vec::<String>::new());
    }

    #[test]
    fn syntax_variants() {
        assert_eq!(conv("10 km in mi"), "6.213711922 mi");
        assert_eq!(conv("10 km to mi"), "6.213711922 mi");
        assert_eq!(conv("10 km as mi"), "6.213711922 mi");
        assert_eq!(conv("10 km = mi"), "6.213711922 mi");
        assert_eq!(conv("10km=mi"), "6.213711922 mi");
        assert_eq!(conv("10 km → mi"), "6.213711922 mi");
        assert_eq!(conv("10 KM IN MI"), "6.213711922 mi");
        assert_eq!(conv("  10   km   in   mi  "), "6.213711922 mi");
        assert_eq!(conv("10 kilometers to miles"), "6.213711922 mi");
    }

    #[test]
    fn expressions_as_the_amount() {
        assert_eq!(conv("(2 + 3) km in m"), "5000 m");
        assert_eq!(conv("2 * 5 km to m"), "10000 m");
        assert_eq!(conv("2^10 B in KiB"), "1 KiB");
        assert_eq!(conv("1e3 m in km"), "1 km");
        assert_eq!(conv("-5 km in m"), "-5000 m");
        assert_eq!(conv("1_000 m in km"), "1 km");
        assert_eq!(conv(".5 km in m"), "500 m");
    }

    #[test]
    fn in_is_both_separator_and_inches() {
        assert_eq!(conv("12 in in cm"), "30.48 cm");
        assert_eq!(conv("1 ft in in"), "12 in");
        assert_eq!(conv("5 in to cm"), "12.7 cm");
        assert_eq!(conv("5 ft 11 in in cm"), "180.34 cm");
    }

    #[test]
    fn length() {
        assert_eq!(conv("1 mi in km"), "1.609344 km");
        assert_eq!(conv("3 ft in m"), "0.9144 m");
        assert_eq!(conv("1 yd in ft"), "3 ft");
        assert_eq!(conv("100 cm in m"), "1 m");
        assert_eq!(conv("1 nmi in m"), "1852 m");
        assert_eq!(conv("1 m in mm"), "1000 mm");
        assert_eq!(conv("1 µm in nm"), "1000 nm");
        assert_eq!(conv("1 um in nm"), "1000 nm");
        assert_eq!(conv("1 inch to mm"), "25.4 mm");
        assert_eq!(conv("2 feet to inches"), "24 in");
        close("1 ly in km", 9.4607304725808e12);
        close("1 au in km", 149_597_870.7);
    }

    #[test]
    fn mass() {
        assert_eq!(conv("1 kg in lb"), "2.204622622 lb");
        assert_eq!(conv("1 lb in oz"), "16 oz");
        assert_eq!(conv("1 lb in g"), "453.59237 g");
        assert_eq!(conv("14 lb in st"), "1 st");
        assert_eq!(conv("1 t in kg"), "1000 kg");
        assert_eq!(conv("1 tonne in kg"), "1000 kg");
        assert_eq!(conv("1 ton in lb"), "2000 lb");
        assert_eq!(conv("1 long ton in lb"), "2240 lb");
        assert_eq!(conv("500 mg in g"), "0.5 g");
        assert_eq!(conv("1 oz in g"), "28.34952313 g");
        assert_eq!(conv("1 ct in mg"), "200 mg");
    }

    #[test]
    fn temperature_is_affine() {
        assert_eq!(conv("0 c to f"), "32 °F");
        assert_eq!(conv("100 c to f"), "212 °F");
        assert_eq!(conv("100 f to c"), "37.77777778 °C");
        assert_eq!(conv("72°F in C"), "22.22222222 °C");
        assert_eq!(conv("32 f to c"), "0 °C");
        assert_eq!(conv("-40 f to c"), "-40 °C");
        assert_eq!(conv("-40 c to f"), "-40 °F");
        assert_eq!(conv("0 c to k"), "273.15 K");
        assert_eq!(conv("0 k to c"), "-273.15 °C");
        assert_eq!(conv("300 K in °C"), "26.85 °C");
        assert_eq!(conv("0 k to f"), "-459.67 °F");
        assert_eq!(conv("491.67 rankine to c"), "0 °C");
        assert_eq!(conv("98.6 degrees fahrenheit to celsius"), "37 °C");
        assert_eq!(conv("20 deg C to F"), "68 °F");
        assert_eq!(conv("20 degC in degF"), "68 °F");
        assert_eq!(conv("20 °C = °F"), "68 °F");
    }

    #[test]
    fn temperature_has_no_compound_form() {
        assert!(answer("1 c 2 c in f").is_none());
    }

    #[test]
    fn volume() {
        assert_eq!(conv("1 L in ml"), "1000 mL");
        assert_eq!(conv("1000 ml to l"), "1 L");
        close("3.5 cups to ml", 828.05882775);
        assert_eq!(conv("1 gal in L"), "3.785411784 L");
        assert_eq!(conv("1 gal in qt"), "4 qt");
        assert_eq!(conv("1 qt in pt"), "2 pt");
        assert_eq!(conv("1 pt in cups"), "2 cup");
        assert_eq!(conv("1 cup in fl oz"), "8 fl oz");
        assert_eq!(conv("1 tbsp in tsp"), "3 tsp");
        assert_eq!(conv("1 fl oz in ml"), "29.57352956 mL");
        assert_eq!(conv("1 imp gal in L"), "4.54609 L");
        assert_eq!(conv("1 imp gal in imp pt"), "8 imp pt");
        assert_eq!(conv("1 cc in ml"), "1 mL");
        assert_eq!(conv("1 m3 in L"), "1000 L");
        assert_eq!(conv("1 cubic meter to liters"), "1000 L");
        assert_eq!(conv("1 cm³ to ml"), "1 mL");
        assert_eq!(conv("1 ft3 in L"), "28.31684659 L");
    }

    #[test]
    fn area() {
        assert_eq!(conv("1 m2 in cm2"), "10000 cm²");
        assert_eq!(conv("1 m^2 in ft2"), "10.76391042 ft²");
        assert_eq!(conv("1 sq ft in sq in"), "144 in²");
        assert_eq!(conv("1 square mile to acres"), "640 ac");
        assert_eq!(conv("1 ha in m²"), "10000 m²");
        assert_eq!(conv("1 acre to sqft"), "43560 ft²");
        assert_eq!(conv("1 km2 to ha"), "100 ha");
        assert_eq!(conv("2 square feet in square inches"), "288 in²");
    }

    #[test]
    fn speed() {
        assert_eq!(conv("60 mph in km/h"), "96.56064 km/h");
        assert_eq!(conv("100 km/h to mph"), "62.13711922 mph");
        assert_eq!(conv("10 m/s to km/h"), "36 km/h");
        assert_eq!(conv("1 knot in km/h"), "1.852 km/h");
        assert_eq!(conv("100 kph in mph"), "62.13711922 mph");
        assert_eq!(conv("60 miles per hour to m/s"), "26.8224 m/s");
        assert_eq!(conv("1 ft/s to cm / s"), "30.48 cm/s");
        assert_eq!(conv("1 mi/h in mph"), "1 mph");
        assert_eq!(conv("1 km/min in m/s"), "16.66666667 m/s");
    }

    #[test]
    fn data_si_and_iec() {
        assert_eq!(conv("5 GB in MiB"), "4768.371582 MiB");
        assert_eq!(conv("1 GiB in MB"), "1073.741824 MB");
        assert_eq!(conv("1 GB in MB"), "1000 MB");
        assert_eq!(conv("1 MiB in KiB"), "1024 KiB");
        assert_eq!(conv("1 TB in GB"), "1000 GB");
        assert_eq!(conv("1 TiB in GiB"), "1024 GiB");
        assert_eq!(conv("1 byte in bits"), "8 bit");
        assert_eq!(conv("1 KB in B"), "1000 B");
        assert_eq!(conv("1 kB in B"), "1000 B");
        assert_eq!(conv("1 MB in kB"), "1000 kB");
        assert_eq!(conv("1 PB in TB"), "1000 TB");
        assert_eq!(conv("1 megabyte to kilobytes"), "1000 kB");
        assert_eq!(conv("1 mebibyte to kibibytes"), "1024 KiB");
    }

    #[test]
    fn data_case_picks_bits_or_bytes() {
        assert_eq!(unit("MB").symbol, "MB");
        assert_eq!(unit("Mb").symbol, "Mbit");
        assert_eq!(unit("mb").symbol, "MB");
        assert_eq!(unit("gb").symbol, "GB");
        assert_eq!(unit("Gb").symbol, "Gbit");
        assert_eq!(unit("B").symbol, "B");
        assert_eq!(unit("b").symbol, "bit");
        assert_eq!(unit("KiB").symbol, "KiB");
        assert_eq!(unit("Kib").symbol, "Kibit");
        assert_eq!(unit("kib").symbol, "KiB");
        assert_eq!(conv("8 Mb in MB"), "1 MB");
        assert_eq!(conv("1 MB in Mb"), "8 Mbit");
        assert_eq!(conv("100 Mbit to MB"), "12.5 MB");
        assert_eq!(conv("1 Gb in Mb"), "1000 Mbit");
        assert_eq!(conv("1 Gib in Mib"), "1024 Mibit");
    }

    #[test]
    fn time() {
        assert_eq!(conv("90 min in h"), "1.5 h");
        assert_eq!(conv("1 h in s"), "3600 s");
        assert_eq!(conv("1 day in hours"), "24 h");
        assert_eq!(conv("1 week in days"), "7 d");
        assert_eq!(conv("1 yr in days"), "365.25 d");
        assert_eq!(conv("12 mo in yr"), "1 yr");
        assert_eq!(conv("1 s in ms"), "1000 ms");
        assert_eq!(conv("1 ms in µs"), "1000 µs");
        assert_eq!(conv("1 ms in us"), "1000 µs");
        assert_eq!(conv("1 ms in ns"), "1000000 ns");
        assert_eq!(conv("2 hrs to min"), "120 min");
        assert_eq!(conv("1 decade in years"), "10 yr");
    }

    #[test]
    fn compound_quantities() {
        assert_eq!(conv("2 h 30 min in min"), "150 min");
        assert_eq!(conv("2h30min to s"), "9000 s");
        assert_eq!(conv("1 h 30 min 15 s in s"), "5415 s");
        assert_eq!(conv("5'11\" to cm"), "180.34 cm");
        assert_eq!(conv("5' 11\" in cm"), "180.34 cm");
        assert_eq!(conv("6' to cm"), "182.88 cm");
        assert_eq!(conv("70\" to cm"), "177.8 cm");
        assert_eq!(conv("5 ft 11 in to m"), "1.8034 m");
        assert_eq!(conv("1 lb 8 oz to g"), "680.388555 g");
        assert_eq!(conv("-1 h 30 min in min"), "-90 min");
        assert!(answer("1 h 30 kg in min").is_none());
    }

    #[test]
    fn pressure() {
        assert_eq!(conv("1 atm in Pa"), "101325 Pa");
        assert_eq!(conv("1 atm in kPa"), "101.325 kPa");
        assert_eq!(conv("1 bar in kPa"), "100 kPa");
        assert_eq!(conv("1 bar in psi"), "14.50377377 psi");
        assert_eq!(conv("14.7 psi to atm"), "1.000275669 atm");
        assert_eq!(conv("760 torr in atm"), "1 atm");
        assert_eq!(conv("1 MPa in bar"), "10 bar");
        assert_eq!(conv("1 hPa in mbar"), "1 mbar");
        close("760 mmHg in kPa", 101.325);
        close("1 inHg in kPa", 3.386388640);
    }

    #[test]
    fn energy() {
        assert_eq!(conv("1 kcal in kJ"), "4.184 kJ");
        assert_eq!(conv("1 kcal in cal"), "1000 cal");
        assert_eq!(conv("1 Cal in cal"), "1000 cal");
        assert_eq!(conv("1 kWh in MJ"), "3.6 MJ");
        assert_eq!(conv("1 kWh in J"), "3600000 J");
        assert_eq!(conv("1 Wh in J"), "3600 J");
        assert_eq!(conv("1 BTU in J"), "1055.055853 J");
        assert_eq!(conv("1000 kJ in kWh"), "0.2777777778 kWh");
        close("1 eV in J", 1.602176634e-19);
    }

    #[test]
    fn angle() {
        assert_eq!(conv("180 deg in rad"), "3.141592654 rad");
        assert_eq!(conv("180 degrees to rad"), "3.141592654 rad");
        assert_eq!(conv("180° to rad"), "3.141592654 rad");
        assert_eq!(conv("1 turn in deg"), "360°");
        assert_eq!(conv("90 deg in grad"), "100 grad");
        assert_eq!(conv("1 deg in arcmin"), "60 arcmin");
        assert_eq!(conv("1 arcmin in arcsec"), "60 arcsec");
        assert_eq!(conv("2 * 3.141592653589793 rad in deg"), "360°");
        assert_eq!(conv("0.5 rev to deg"), "180°");
    }

    #[test]
    fn plurals_and_spellings() {
        assert_eq!(unit("miles").symbol, "mi");
        assert_eq!(unit("Miles").symbol, "mi");
        assert_eq!(unit("inches").symbol, "in");
        assert_eq!(unit("feet").symbol, "ft");
        assert_eq!(unit("foot").symbol, "ft");
        assert_eq!(unit("pounds").symbol, "lb");
        assert_eq!(unit("kgs").symbol, "kg");
        assert_eq!(unit("kms").symbol, "km");
        assert_eq!(unit("hours").symbol, "h");
        assert_eq!(unit("litres").symbol, "L");
        assert_eq!(unit("fl. oz.").symbol, "fl oz");
        assert_eq!(unit("fluid  ounces").symbol, "fl oz");
        assert_eq!(unit("kilowatt-hours").symbol, "kWh");
        assert_eq!(unit("m²").symbol, "m²");
        assert_eq!(unit("m^3").symbol, "m³");
        assert_eq!(unit("Kelvin").symbol, "K");
        assert_eq!(unit("ºC").symbol, "°C");
    }

    #[test]
    fn single_letters_are_not_pluralized_away() {
        // `ms` must stay milliseconds, not become `m` + plural.
        assert_eq!(unit("ms").symbol, "ms");
        assert!(lookup("bs").is_none());
        assert!(lookup("xs").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("   ").is_none());
    }

    #[test]
    fn the_examples_in_the_readme_convert() {
        for input in [
            "10 km in mi",
            "5'11\" to cm",
            "3 ft 4 in to cm",
            "1 kg in lb",
            "8 oz to g",
            "1 ton in kg",
            "100 f to c",
            "72°F in C",
            "0 c to k",
            "3.5 cups to ml",
            "1 gal in L",
            "1 tbsp in tsp",
            "1 imp gal in L",
            "2 m3 in L",
            "1 acre in m2",
            "500 sq ft to m²",
            "1 ha in acres",
            "60 mph in km/h",
            "10 m/s to knots",
            "5 GB in MiB",
            "100 Mbit to MB",
            "1 TiB in GB",
            "2 h 30 min in min",
            "90 min to h",
            "1 yr in days",
            "1 atm in psi",
            "1 bar in kPa",
            "760 torr in atm",
            "1 kcal in kJ",
            "1 kWh in MJ",
            "180 deg in rad",
            "1 turn in deg",
            "(2+3) km in m",
            "12 in in cm",
        ] {
            assert!(answer(input).is_some(), "`{input}` should convert");
        }
    }

    #[test]
    fn rounding_hides_float_noise() {
        assert_eq!(conv("0.1 km in m"), "100 m");
        assert_eq!(conv("0.3 m in cm"), "30 cm");
        assert_eq!(conv("1.1 m in mm"), "1100 mm");
        assert_eq!(conv("3 ft in m"), "0.9144 m");
        assert_eq!(conv("1 mi in ft"), "5280 ft");
        assert_eq!(conv("1 ft in mm"), "304.8 mm");
        assert_eq!(conv("1 yd in in"), "36 in");
        assert_eq!(conv("1/3 km in m"), "333.3333333 m");
        assert_eq!(conv("1 m in km"), "0.001 km");
        assert_eq!(conv("1 nm in km"), "1e-12 km");
        assert_eq!(conv("1 ly in m"), "9.460730473e+15 m");
        assert_eq!(conv("0 km in mi"), "0 mi");
    }

    #[test]
    fn same_unit_is_identity() {
        assert_eq!(conv("5 km in km"), "5 km");
        assert_eq!(conv("5 c in c"), "5 °C");
        assert_eq!(conv("-3.25 f in f"), "-3.25 °F");
    }

    #[test]
    fn different_categories_do_not_convert() {
        assert!(answer("10 km in kg").is_none());
        assert!(answer("10 c in km").is_none());
        assert!(answer("10 MB in s").is_none());
        assert!(answer("10 m2 in m3").is_none());
        assert!(answer("10 m in m2").is_none());
        let km = unit("km");
        let kg = unit("kg");
        assert!(convert(1.0, &km, &kg).is_none());
    }

    #[test]
    fn unrelated_input_is_ignored() {
        for input in [
            "",
            "10",
            "10 km",
            "km in mi",
            "in",
            "to",
            "10 in",
            "10 km in",
            "10 km to nowhere",
            "firefox",
            "go to the shop",
            "2+2",
            "1 + 2 in m",
            "chrome as admin",
            "10 apples in oranges",
            "= 5",
            "10 km in mi in",
            "5 to 6",
        ] {
            assert!(answer(input).is_none(), "`{input}` should not convert");
        }
    }

    #[test]
    fn overflow_is_not_an_answer() {
        assert!(answer("1e300 ly in nm").is_none());
        assert!(convert(f64::INFINITY, &unit("m"), &unit("km")).is_none());
    }

    #[test]
    fn conversion_reports_the_input_for_the_subtitle() {
        let c = answer("10  km  in  mi").unwrap();
        assert_eq!(c.input, "10  km → mi");
        assert_eq!(c.text, "6.213711922 mi");
    }

    #[test]
    fn split_conversion_prefers_the_last_separator() {
        assert_eq!(
            split_conversion("12 in in cm"),
            vec![("12 in", "cm"), ("12", "in cm")]
        );
        assert_eq!(split_conversion("a=b"), vec![("a", "b")]);
        assert!(split_conversion("nothing here").is_empty());
        assert!(split_conversion("in cm").is_empty());
        assert!(split_conversion("10 into cm").is_empty());
    }

    #[test]
    fn split_quantity_finds_unit_boundaries() {
        assert_eq!(split_quantity("72°F"), vec![(72.0, "°F")]);
        assert_eq!(split_quantity("(2+3) km"), vec![(5.0, "km")]);
        assert_eq!(split_quantity("10"), vec![]);
        assert_eq!(split_quantity("km"), vec![]);
        assert_eq!(split_quantity("2 h 30 min"), vec![(2.0, "h 30 min")]);
    }

    #[test]
    fn round_significant_cleans_noise() {
        assert_eq!(round_significant(0.1 + 0.2, 10), 0.3);
        assert_eq!(round_significant(0.0, 10), 0.0);
        assert_eq!(round_significant(1234.56789, 3), 1230.0);
        assert!(round_significant(f64::NAN, 5).is_nan());
    }
}
