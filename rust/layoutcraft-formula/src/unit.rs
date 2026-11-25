//! Units of measure and the dimension bookkeeping behind them.
//!
//! Sources: Microsoft Learn, "About Units of Measure (Visio ShapeSheet Reference)" (abbreviations,
//! implicit units) and "About Date, Time, and Duration Values" (durations). Internally lengths are
//! inches, angles radians and durations days, as the documentation states.

/// Exponents of the three base dimensions a value can carry.
///
/// A length has `length == 1`, an area `length == 2`, an angle `angle == 1`, a duration
/// `time == 1`. Multiplying adds exponents, dividing subtracts them. The documentation limits the
/// exponent to `-128..=127`, which is exactly the `i8` range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Dim {
    pub length: i8,
    pub angle: i8,
    pub time: i8,
}

impl Dim {
    pub const NONE: Dim = Dim {
        length: 0,
        angle: 0,
        time: 0,
    };
    pub const LENGTH: Dim = Dim {
        length: 1,
        angle: 0,
        time: 0,
    };
    pub const AREA: Dim = Dim {
        length: 2,
        angle: 0,
        time: 0,
    };
    pub const ANGLE: Dim = Dim {
        length: 0,
        angle: 1,
        time: 0,
    };
    pub const DURATION: Dim = Dim {
        length: 0,
        angle: 0,
        time: 1,
    };

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }

    /// Sum of exponents; `None` when one leaves the documented range.
    pub fn checked_add(self, other: Dim) -> Option<Dim> {
        Some(Dim {
            length: self.length.checked_add(other.length)?,
            angle: self.angle.checked_add(other.angle)?,
            time: self.time.checked_add(other.time)?,
        })
    }

    pub fn checked_sub(self, other: Dim) -> Option<Dim> {
        Some(Dim {
            length: self.length.checked_sub(other.length)?,
            angle: self.angle.checked_sub(other.angle)?,
            time: self.time.checked_sub(other.time)?,
        })
    }

    /// Exponents multiplied by an integer power.
    pub fn checked_scale(self, power: i32) -> Option<Dim> {
        let scale = |exponent: i8| -> Option<i8> {
            i8::try_from(i32::from(exponent).checked_mul(power)?).ok()
        };
        Some(Dim {
            length: scale(self.length)?,
            angle: scale(self.angle)?,
            time: scale(self.time)?,
        })
    }
}

/// A unit that can follow a number in a formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Inch,
    Foot,
    Yard,
    Mile,
    NauticalMile,
    Millimeter,
    Centimeter,
    Meter,
    Kilometer,
    Point,
    Pica,
    Didot,
    Cicero,
    Degree,
    Radian,
    ArcMinute,
    ArcSecond,
    ElapsedWeek,
    ElapsedDay,
    ElapsedHour,
    ElapsedMinute,
    ElapsedSecond,
    Acre,
    Hectare,
}

/// Metres per inch (exact by definition of the international inch).
const METERS_PER_INCH: f64 = 0.0254;

impl Unit {
    /// Dimension of a value in this unit.
    pub fn dim(self) -> Dim {
        use Unit::*;
        match self {
            Inch | Foot | Yard | Mile | NauticalMile | Millimeter | Centimeter | Meter
            | Kilometer | Point | Pica | Didot | Cicero => Dim::LENGTH,
            Acre | Hectare => Dim::AREA,
            Degree | Radian | ArcMinute | ArcSecond => Dim::ANGLE,
            ElapsedWeek | ElapsedDay | ElapsedHour | ElapsedMinute | ElapsedSecond => Dim::DURATION,
        }
    }

    /// Multiplier from this unit to the internal one (inches, radians, days; square inches for areas).
    pub fn factor(self) -> f64 {
        use std::f64::consts::PI;
        use Unit::*;
        match self {
            Inch => 1.0,
            Foot => 12.0,
            Yard => 36.0,
            Mile => 63_360.0,
            // 1852 m, the international definition.
            NauticalMile => 1852.0 / METERS_PER_INCH,
            Millimeter => 0.001 / METERS_PER_INCH,
            Centimeter => 0.01 / METERS_PER_INCH,
            Meter => 1.0 / METERS_PER_INCH,
            Kilometer => 1000.0 / METERS_PER_INCH,
            Point => 1.0 / 72.0,
            Pica => 1.0 / 6.0,
            // Unspecified in the sources: the Didot point is taken as 1/2660 m; a cicero is 12 didots.
            Didot => 1.0 / (2660.0 * METERS_PER_INCH),
            Cicero => 12.0 / (2660.0 * METERS_PER_INCH),
            Degree => PI / 180.0,
            Radian => 1.0,
            ArcMinute => PI / (180.0 * 60.0),
            ArcSecond => PI / (180.0 * 3600.0),
            ElapsedWeek => 7.0,
            ElapsedDay => 1.0,
            ElapsedHour => 1.0 / 24.0,
            ElapsedMinute => 1.0 / 1440.0,
            ElapsedSecond => 1.0 / 86_400.0,
            // 1 acre = 43 560 square feet.
            Acre => 43_560.0 * 144.0,
            Hectare => 10_000.0 / (METERS_PER_INCH * METERS_PER_INCH),
        }
    }

    /// Looks up a unit by its abbreviation or spelled-out name, ignoring ASCII case.
    ///
    /// The implicit units `DL`, `DP`, `DT`, `DA`, `DE` ("internal units" for drawing, page, type,
    /// angle and time) map to inches, radians and days.
    pub fn parse(name: &str) -> Option<Unit> {
        use Unit::*;
        let lower = name.to_lowercase();
        let name = lower.strip_suffix('.').unwrap_or(&lower);
        Some(match name {
            "in" | "inch" | "inches" | "in_f" | "dl" | "dp" | "dt" => Inch,
            "ft" | "foot" | "feet" => Foot,
            "yd" | "yard" | "yards" => Yard,
            "mi" | "mile" | "miles" | "mi_f" => Mile,
            "nm" => NauticalMile,
            "mm" => Millimeter,
            "cm" => Centimeter,
            "m" => Meter,
            "km" => Kilometer,
            "pt" | "point" | "points" => Point,
            "p" | "pica" | "picas" => Pica,
            "d" | "didot" | "didots" => Didot,
            "c" | "cicero" | "ciceros" => Cicero,
            "deg" | "degree" | "degrees" | "\u{b0}" => Degree,
            "rad" | "radian" | "radians" | "da" => Radian,
            "ew" | "eweek" => ElapsedWeek,
            "ed" | "eday" | "de" => ElapsedDay,
            "eh" | "ehour" => ElapsedHour,
            "em" | "eminute" => ElapsedMinute,
            "es" | "esecond" => ElapsedSecond,
            "acre" | "acres" => Acre,
            "hectare" | "hectares" | "ha" => Hectare,
            _ => return None,
        })
    }
}
