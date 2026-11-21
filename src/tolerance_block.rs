//! A general tolerance given as a block (#228; proposal specify-0.3.0, "A general tolerance
//! given as a block"): the title-block lines of a drawing that names no ISO class, such as
//! `.XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°`, taken exactly as stated.
//!
//! **Entry.** One line of text, its lines separated by `,`, `;`, `·` or new lines. Each line is
//! one of:
//! - decimal places and their tolerance: `.XX ±.01` (inch notation), `X.XX ±0.05` or `X ±0.5`,
//!   with an optional unit (`in`, `"`, `mm`);
//! - fractions: `fractions ±1/64` (or `±1/64`);
//! - angles: `angles ±1°`, `angular ±0°30'`.
//!
//! Anything else is refused, naming the line, so a typo never becomes part of the block.
//!
//! **Units** are stored as stated, never converted (Paul's decision 3): a unit written on any
//! line is the block's; else inch notation (`.XX`) or a fraction means inches; else the block
//! is in the file's length unit.
//!
//! **In the file** (Paul's decision 1) the decimal-place lines are AP242's
//! `default_tolerance_table`, one `default_tolerance_table_cell` each: its 'significant number
//! of digits' (the places), its 'plus minus tolerance value' in the block's unit, and a 'cell
//! description' with the line as stated. A line the table cannot express (fractions and
//! angles: a cell needs a number of digits or a range, WR3) goes, as stated, in the general
//! tolerance note beside the table.
//!
//! **Read back** ([`table_text`]) from the table alone, whoever wrote it: decimal places or
//! ranges with their tolerance, in the order of the cells, then the note's lines. Nothing is
//! worked out from the block (what ±.005 means for a dimension written with two places).

use std::sync::LazyLock;

use haecceity::pmi::{
    AngleUnit, CellValue, Decimal, GeneralTolerance, LengthUnit, Quantity, ToleranceCell, Value,
};
use regex::Regex;

/// The line separator a block is shown with.
pub const SEPARATOR: &str = " · ";

/// The line separator in the general tolerance note, whose text is drawn (PMI fonts have no
/// `·`).
const NOTE_SEPARATOR: &str = "; ";

/// One line of a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// Dimensions written with `places` decimal places are toleranced `±tolerance`. `inch_style`:
    /// written `.XX` rather than `X.XX`.
    Places {
        places: u8,
        tolerance: Decimal,
        inch_style: bool,
    },
    /// A fractional tolerance, as stated (`±1/64`).
    Fraction(String),
    /// An angular tolerance, as stated (`angles ±1°`).
    Angle(String),
}

/// A block as entered: its lines in order, and the unit it states, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub lines: Vec<Line>,
    /// The unit written on a line (`in`, `mm`), applying to every length line.
    pub unit: Option<LengthUnit>,
}

/// What a block writes into the file: the table (when it has decimal-place lines) and the
/// note (the lines the table cannot express).
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub table: Option<GeneralTolerance>,
    pub note: Option<String>,
}

const PM: &str = r"(?:±|\+/-|\+-)";
const NUMBER: &str = r"([0-9]+(?:\.[0-9]*)?|\.[0-9]+)";
const UNIT: &str = r#"(in|inch|inches|"|mm)"#;

fn regex(text: &str) -> Regex {
    Regex::new(&format!("(?i){text}")).expect("a valid pattern")
}

static PLACES: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r"^(?:(\.)(x+)|x(?:\.(x+))?)\s*{PM}\s*{NUMBER}\s*{UNIT}?$"
    ))
});
static FRACTION: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r"^(?:fractions?|fractional|x/x)?\s*:?\s*{PM}\s*([0-9]+)\s*/\s*([0-9]+)\s*{UNIT}?$"
    ))
});
static ANGLE: LazyLock<Regex> = LazyLock::new(|| {
    regex(&format!(
        r#"^(?:angles?|angular)?\s*:?\s*{PM}\s*([0-9]+(?:\.[0-9]+)?)\s*(?:°|deg|degrees?)\s*(?:([0-9]+)\s*(?:′|'|min)\s*)?(?:([0-9]+)\s*(?:″|"|sec)\s*)?$"#
    ))
});

fn unit_of(text: &str) -> LengthUnit {
    if text.eq_ignore_ascii_case("mm") {
        LengthUnit::Millimetre
    } else {
        LengthUnit::Inch
    }
}

/// `.01` as a Part 21 decimal (`0.01`), digits otherwise as typed.
fn decimal(text: &str) -> Option<Decimal> {
    let text = text.trim_end_matches('.');
    let text = if text.starts_with('.') {
        format!("0{text}")
    } else {
        text.to_string()
    };
    Decimal::parse(&text).ok().filter(|d| !d.is_zero())
}

/// The block `text` states, or why it is not one. `Ok(None)` when no line of it is a block line
/// (an ISO class, a ± figure): it is not a block at all.
pub fn parse(text: &str) -> Result<Option<Block>, String> {
    let pieces: Vec<&str> = text
        .split([',', ';', '·', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let mut lines = Vec::new();
    let mut unit: Option<LengthUnit> = None;
    // Each line not read, with why.
    let mut bad: Vec<(String, &str)> = Vec::new();
    let mut state = |u: Option<regex::Match<'_>>, bad: &mut Vec<(String, &str)>, piece: &str| {
        if let Some(u) = u {
            let u = unit_of(u.as_str());
            match &unit {
                Some(old) if *old != u => {
                    bad.push((piece.to_string(), "a second unit: a block has one"));
                }
                _ => unit = Some(u),
            }
        }
    };
    let mut places_seen: Vec<u8> = Vec::new();
    for piece in &pieces {
        let squashed = piece.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(c) = PLACES.captures(&squashed) {
            let inch_style = c.get(1).is_some();
            let xs = c.get(2).or(c.get(3)).map_or(0, |m| m.as_str().len());
            let places = u8::try_from(xs).unwrap_or(u8::MAX);
            let Some(tolerance) = decimal(&c[4]) else {
                bad.push((piece.to_string(), "a tolerance of zero"));
                continue;
            };
            if places_seen.contains(&places) {
                bad.push((
                    piece.to_string(),
                    "the same number of decimal places is stated twice",
                ));
                continue;
            }
            places_seen.push(places);
            state(c.get(5), &mut bad, piece);
            lines.push(Line::Places {
                places,
                tolerance,
                inch_style,
            });
        } else if let Some(c) = FRACTION.captures(&squashed) {
            let (n, d) = (&c[1], &c[2]);
            if d.trim_start_matches('0').is_empty() || n.trim_start_matches('0').is_empty() {
                bad.push((piece.to_string(), "a fraction with a zero in it"));
                continue;
            }
            if lines.iter().any(|l| matches!(l, Line::Fraction(_))) {
                bad.push((piece.to_string(), "fractions are stated twice"));
                continue;
            }
            state(c.get(3), &mut bad, piece);
            lines.push(Line::Fraction(format!("±{n}/{d}")));
        } else if let Some(c) = ANGLE.captures(&squashed) {
            let zero = |m: Option<regex::Match<'_>>| {
                m.is_none_or(|m| m.as_str().chars().all(|ch| ch == '0' || ch == '.'))
            };
            if zero(c.get(1)) && zero(c.get(2)) && zero(c.get(3)) {
                bad.push((piece.to_string(), "a tolerance of zero"));
                continue;
            }
            if lines.iter().any(|l| matches!(l, Line::Angle(_))) {
                bad.push((piece.to_string(), "angles are stated twice"));
                continue;
            }
            let mut text = format!("angles ±{}°", &c[1]);
            if let Some(m) = c.get(2) {
                text.push_str(&format!("{}′", m.as_str()));
            }
            if let Some(s) = c.get(3) {
                text.push_str(&format!("{}″", s.as_str()));
            }
            lines.push(Line::Angle(text));
        } else {
            bad.push((piece.to_string(), why_not(&squashed)));
        }
    }
    // Text that is no block at all (an ISO class, a ± figure, none) is left to the other
    // readings; text that looks like a block but does not read is refused, saying why.
    if lines.is_empty() && !pieces.iter().any(|p| LOOKS.is_match(p)) {
        return Ok(None);
    }
    if !bad.is_empty() {
        let shown: Vec<String> = bad
            .iter()
            .map(|(piece, why)| format!("'{piece}': {why}"))
            .collect();
        return Err(format!(
            "Not read as a general tolerance block: {}. Each line, separated by commas, is \
             decimal places and their tolerance (.XX ±.01, X.X ±0.2), fractions ±1/64 or \
             angles ±1°.",
            shown.join("; ")
        ));
    }
    Ok(Some(Block { lines, unit }))
}

/// Text that looks like a line of a block: decimal-place notation, fractions or angles with a
/// tolerance (a bare `X` is not).
static LOOKS: LazyLock<Regex> =
    LazyLock::new(|| regex(r"^(?:\.?x|fraction|angl).*(?:±|\+-|\+/-|°)|°"));

/// Why a line that matches no form is not read.
fn why_not(line: &str) -> &'static str {
    let tolerances = line.matches('±').count() + line.matches("+-").count();
    if tolerances > 1 {
        "two tolerances on one line: separate the lines with commas"
    } else if regex(r"^x{2,}\b").is_match(line) {
        "decimal places are written with a point: .XX (inch) or X.XX"
    } else if tolerances == 0 {
        "no ± tolerance"
    } else {
        "not decimal places with a tolerance, fractions or angles"
    }
}

impl Block {
    /// The unit of the block's length lines in a file whose length unit is `file_unit`: the
    /// unit stated, else inches for inch notation or a fraction, else the file's.
    pub fn unit_in(&self, file_unit: &LengthUnit) -> LengthUnit {
        match &self.unit {
            Some(u) => u.clone(),
            None if self.inch_notation() => LengthUnit::Inch,
            None => file_unit.clone(),
        }
    }

    /// Whether a line is in inch notation (`.XX`) or a fraction.
    fn inch_notation(&self) -> bool {
        self.lines.iter().any(|l| {
            matches!(
                l,
                Line::Places {
                    inch_style: true,
                    ..
                } | Line::Fraction(_)
            )
        })
    }

    /// The block's text as entered, in its canonical spelling: decimal-place lines first, then
    /// the others, each in the order given. A unit is named only where it was stated and the
    /// notation does not imply it.
    #[must_use]
    pub fn text(&self) -> String {
        let resolved = self
            .unit
            .clone()
            .or_else(|| self.inch_notation().then_some(LengthUnit::Inch));
        // The unit is named whenever the notation alone does not imply it: inches without a
        // `.XX` line or a fraction (`X ±.5 in`), and any other unit stated.
        let suffix = match &resolved {
            None => String::new(),
            Some(LengthUnit::Inch) if self.inch_notation() => String::new(),
            Some(u) => format!(" {}", unit_name(u)),
        };
        let inch = resolved == Some(LengthUnit::Inch);
        let mut out: Vec<String> = Vec::new();
        for l in &self.lines {
            if let Line::Places {
                places, tolerance, ..
            } = l
            {
                out.push(places_line(*places, None, tolerance, inch, &suffix));
            }
        }
        for l in &self.lines {
            match l {
                Line::Fraction(t) => out.push(if inch {
                    t.clone()
                } else {
                    format!("{t}{suffix}")
                }),
                // (A fraction implies inches, so an inch block with one names no unit.)
                Line::Angle(t) => out.push(t.clone()),
                Line::Places { .. } => {}
            }
        }
        out.join(SEPARATOR)
    }

    /// What the block writes into a file whose length unit is `file_unit`.
    pub fn written(&self, file_unit: &LengthUnit) -> Result<Written, String> {
        let unit = self.unit_in(file_unit);
        let inch = unit == LengthUnit::Inch;
        let mut cells = Vec::new();
        let mut note = Vec::new();
        for l in &self.lines {
            match l {
                Line::Places {
                    places, tolerance, ..
                } => {
                    let digits = Decimal::parse(&places.to_string()).map_err(|e| e.0)?;
                    let stated = places_line(*places, None, tolerance, inch, "");
                    cells.push(ToleranceCell {
                        items: vec![
                            (
                                "significant number of digits".into(),
                                CellValue::Count(digits),
                            ),
                            (
                                "plus minus tolerance value".into(),
                                CellValue::Value(Value::length(tolerance.clone(), unit.clone())),
                            ),
                            ("cell description".into(), CellValue::Text(stated)),
                        ],
                    });
                }
                Line::Fraction(t) if !inch => note.push(format!("{t} {}", unit_name(&unit))),
                Line::Fraction(t) => note.push(t.clone()),
                // The note's text is drawn too: minutes and seconds in the characters PMI
                // fonts have.
                Line::Angle(t) => note.push(t.replace('′', "'").replace('″', "\"")),
            }
        }
        Ok(Written {
            table: (!cells.is_empty()).then(|| GeneralTolerance::Table {
                name: String::new(),
                cells,
            }),
            note: (!note.is_empty()).then(|| note.join(NOTE_SEPARATOR)),
        })
    }
}

fn unit_name(u: &LengthUnit) -> String {
    match u {
        LengthUnit::Millimetre => "mm".into(),
        LengthUnit::Micrometre => "µm".into(),
        LengthUnit::Centimetre => "cm".into(),
        LengthUnit::Metre => "m".into(),
        LengthUnit::Inch => "in".into(),
        LengthUnit::Foot => "ft".into(),
        LengthUnit::Other { name, .. } => name.clone(),
    }
}

/// `0.01` in inches as a drawing writes it (`.01`); any other unit as stated, a Part 21
/// integer's point (`6.`) left off.
fn number(d: &Decimal, inch: bool) -> String {
    let t = d.as_str();
    let t = t.strip_suffix('.').unwrap_or(t);
    if inch {
        if let Some(rest) = t.strip_prefix("0.") {
            return format!(".{rest}");
        }
        if let Some(rest) = t.strip_prefix("-0.") {
            return format!("-.{rest}");
        }
    }
    t.to_string()
}

fn places_line(
    places: u8,
    range: Option<String>,
    tolerance: &Decimal,
    inch: bool,
    suffix: &str,
) -> String {
    let xs = "X".repeat(usize::from(places));
    let notation = range.unwrap_or_else(|| match (inch, places) {
        (_, 0) => "X".into(),
        (true, _) => format!(".{xs}"),
        (false, _) => format!("X.{xs}"),
    });
    format!("{notation} ±{}{suffix}", number(tolerance, inch))
}

/// A cell's value as text, with its unit when it is not `file_unit` (inches implied by `inch`).
fn value_text(v: &Value, file_unit: &LengthUnit, inch_implied: bool) -> String {
    match &v.quantity {
        Quantity::Length(l) => {
            let inch = l.unit == LengthUnit::Inch;
            let n = number(&l.value, inch);
            if l.unit == *file_unit || (inch && inch_implied) {
                n
            } else {
                format!("{n} {}", unit_name(&l.unit))
            }
        }
        Quantity::Angle(a) => match &a.unit {
            AngleUnit::Degree => format!("{}°", a.value.as_str()),
            AngleUnit::Radian => format!("{} rad", a.value.as_str()),
            AngleUnit::Other { name, .. } => format!("{} {name}", a.value.as_str()),
        },
    }
}

/// One table cell as a line, read from its items whoever wrote it; `None` when it has neither a
/// number of digits or a range nor a tolerance.
/// `dotted_inch`: the table has an inch cell shown in inch notation (`.XX`), which implies
/// inches for its other inch cells; otherwise an inch value outside an inch file names its unit.
fn cell_text(cell: &ToleranceCell, file_unit: &LengthUnit, dotted_inch: bool) -> Option<String> {
    let get = |name: &str| cell.items.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let value = |name: &str| match get(name) {
        Some(CellValue::Value(v)) => Some(v),
        _ => None,
    };
    let places = ["significant number of digits", "number of decimal places"]
        .iter()
        .find_map(|n| match get(n) {
            Some(CellValue::Count(d)) => Some(d.clone()),
            _ => None,
        });
    let inch = [
        "plus minus tolerance value",
        "upper tolerance value",
        "lower tolerance value",
    ]
    .iter()
    .filter_map(|n| value(n))
    .any(|v| matches!(&v.quantity, Quantity::Length(l) if l.unit == LengthUnit::Inch));
    let notation = match (&places, value("lower limit"), value("upper limit")) {
        (Some(d), _, _) => {
            let n = d.to_f64();
            if n.fract() == 0. && (0. ..=32.).contains(&n) {
                let n = n as usize;
                let xs = "X".repeat(n);
                match (inch, n) {
                    (_, 0) => "X".to_string(),
                    (true, _) => format!(".{xs}"),
                    (false, _) => format!("X.{xs}"),
                }
            } else {
                format!("{} digits", d.as_str())
            }
        }
        (None, Some(lo), Some(hi)) => format!(
            "{} to {}",
            value_text(lo, file_unit, false),
            value_text(hi, file_unit, false)
        ),
        _ => return None,
    };
    let inch_implied = inch && (dotted_inch || places.as_ref().is_some_and(|d| d.to_f64() >= 1.));
    let tolerance = if let Some(v) = value("plus minus tolerance value") {
        format!("±{}", value_text(v, file_unit, inch_implied))
    } else if let (Some(up), Some(lo)) = (
        value("upper tolerance value"),
        value("lower tolerance value"),
    ) {
        let up = value_text(up, file_unit, inch_implied);
        let lo = value_text(lo, file_unit, inch_implied);
        let signed = |t: String| {
            if t.starts_with('-') || t.starts_with('+') {
                t
            } else {
                format!("+{t}")
            }
        };
        format!("{}/{}", signed(up), signed(lo))
    } else {
        return None;
    };
    Some(format!("{notation} {tolerance}"))
}

/// A cell that is not a line of digits or a range with a tolerance, its items as stated.
fn items_text(cell: &ToleranceCell, file_unit: &LengthUnit) -> String {
    cell.items
        .iter()
        .map(|(name, v)| match v {
            CellValue::Value(v) => format!("{name} {}", value_text(v, file_unit, false)),
            CellValue::Count(d) => format!("{name} {}", d.as_str()),
            CellValue::Text(t) => format!("{name} {t}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `default_tolerance_table`'s lines as stated, read back from its cells (any writer's), in
/// a file whose length unit is `file_unit`; a cell that is not such a line shows its items.
/// `None` for a table without cells.
#[must_use]
pub fn table_text(cells: &[ToleranceCell], file_unit: &LengthUnit) -> Option<String> {
    let dotted_inch = cells.iter().any(|c| {
        let inch = c.items.iter().any(|(_, v)| {
            matches!(v, CellValue::Value(v)
                if matches!(&v.quantity, Quantity::Length(l) if l.unit == LengthUnit::Inch))
        });
        let places = c.items.iter().any(|(n, v)| {
            matches!(v, CellValue::Count(d)
                if (n == "significant number of digits" || n == "number of decimal places")
                    && d.to_f64() >= 1.)
        });
        inch && places
    });
    let lines: Vec<String> = cells
        .iter()
        .map(|c| cell_text(c, file_unit, dotted_inch).unwrap_or_else(|| items_text(c, file_unit)))
        .collect();
    (!lines.is_empty()).then(|| lines.join(SEPARATOR))
}

/// The general tolerance as the row shows it: the table's lines, then the note's (a block's
/// lines in their canonical spelling, any other note as it is).
#[must_use]
pub fn shown(table: Option<&str>, note: Option<&str>) -> Option<String> {
    let note = note.map(canonical);
    let note = note.as_deref();
    match (table, note) {
        (Some(t), Some(n)) => Some(format!("{t}{SEPARATOR}{n}")),
        (Some(t), None) => Some(t.to_string()),
        (None, Some(n)) => Some(n.to_string()),
        (None, None) => None,
    }
}

/// `text` in a block's canonical spelling when it is a block; else unchanged.
#[must_use]
pub fn canonical(text: &str) -> String {
    match parse(text) {
        Ok(Some(block)) => block.text(),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_228_title_block_reads_as_stated() {
        let b = parse(".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°")
            .unwrap()
            .unwrap();
        assert_eq!(b.text(), ".XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°");
        assert_eq!(b.unit_in(&LengthUnit::Millimetre), LengthUnit::Inch);
        let w = b.written(&LengthUnit::Millimetre).unwrap();
        assert_eq!(w.note.as_deref(), Some("±1/64; angles ±1°"));
        let Some(GeneralTolerance::Table { cells, .. }) = &w.table else {
            panic!("a table");
        };
        assert_eq!(cells.len(), 2);
        assert_eq!(
            table_text(cells, &LengthUnit::Millimetre).as_deref(),
            Some(".XX ±.01 · .XXX ±.005")
        );
    }

    #[test]
    fn a_metric_block_takes_the_files_unit() {
        let b = parse("X ±0.5, X.X ±0.2, X.XX ±0.05, angles ±0°30'")
            .unwrap()
            .unwrap();
        assert_eq!(b.text(), "X ±0.5 · X.X ±0.2 · X.XX ±0.05 · angles ±0°30′");
        assert_eq!(b.unit_in(&LengthUnit::Millimetre), LengthUnit::Millimetre);
        let w = b.written(&LengthUnit::Millimetre).unwrap();
        let Some(GeneralTolerance::Table { cells, .. }) = &w.table else {
            panic!("a table");
        };
        assert_eq!(
            table_text(cells, &LengthUnit::Millimetre).as_deref(),
            Some("X ±0.5 · X.X ±0.2 · X.XX ±0.05")
        );
        // The same cells in an inch file name their unit.
        assert_eq!(
            table_text(cells, &LengthUnit::Inch).as_deref(),
            Some("X ±0.5 mm · X.X ±0.2 mm · X.XX ±0.05 mm")
        );
    }

    #[test]
    fn a_stated_unit_is_kept_and_a_typo_is_refused() {
        let b = parse(".XXX ±.005 in, angular ±1°").unwrap().unwrap();
        assert_eq!(b.text(), ".XXX ±.005 · angles ±1°");
        let b = parse("X.XX +/- 0.05 mm").unwrap().unwrap();
        assert_eq!(b.text(), "X.XX ±0.05 mm");
        assert_eq!(b.unit_in(&LengthUnit::Inch), LengthUnit::Millimetre);
        let e = parse(".XX ±.01, .XXX +- .005 inn").unwrap_err();
        assert!(e.contains(".XXX +- .005 inn"), "{e}");
        for (text, why) in [
            (".XX ±.01 .XXX ±.005", "two tolerances on one line"),
            ("XX ±.01, XXX ±.005", "written with a point"),
            (".XX ±0", "a tolerance of zero"),
            ("fractions ±1/0", "a fraction with a zero"),
            (".XX ±.01, .XX ±.02", "stated twice"),
            ("angles ±0°", "a tolerance of zero"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(why) && !e.contains("ISO 2768"), "{text}: {e}");
        }
        assert!(parse(".XX ±.01 in, .XXX ±.005 mm").is_err());
    }

    #[test]
    fn a_class_or_a_figure_is_no_block() {
        for t in ["ISO 2768-m", "±0.3 mm or ±0.3 %", "none", ""] {
            assert_eq!(parse(t), Ok(None), "{t}");
        }
    }
}
