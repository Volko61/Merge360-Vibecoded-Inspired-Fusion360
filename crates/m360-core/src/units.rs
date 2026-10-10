use serde::{Deserialize, Serialize};

/// Document length units. Internally everything is stored in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Units {
    #[default]
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
}

impl Units {
    pub const ALL: [Units; 5] = [
        Units::Millimeter,
        Units::Centimeter,
        Units::Meter,
        Units::Inch,
        Units::Foot,
    ];

    /// Millimetres per unit.
    pub fn scale(self) -> f64 {
        match self {
            Units::Millimeter => 1.0,
            Units::Centimeter => 10.0,
            Units::Meter => 1000.0,
            Units::Inch => 25.4,
            Units::Foot => 304.8,
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Units::Millimeter => "mm",
            Units::Centimeter => "cm",
            Units::Meter => "m",
            Units::Inch => "in",
            Units::Foot => "ft",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Units::Millimeter => "Millimeter",
            Units::Centimeter => "Centimeter",
            Units::Meter => "Meter",
            Units::Inch => "Inch",
            Units::Foot => "Foot",
        }
    }

    pub fn from_mm(self, mm: f64) -> f64 {
        mm / self.scale()
    }

    pub fn to_mm(self, v: f64) -> f64 {
        v * self.scale()
    }

    pub fn format(self, mm: f64) -> String {
        format!("{:.2} {}", self.from_mm(mm), self.suffix())
    }

    /// Parse a user length expression such as `"25"`, `"1 in"`, `"10mm + 2.5"`.
    /// Bare numbers are interpreted in `self`. Returns millimetres.
    pub fn parse(self, text: &str) -> Option<f64> {
        let mut total = 0.0;
        let mut any = false;
        let normalized = text.replace('-', "+-");
        for term in normalized.split('+') {
            let term = term.trim();
            if term.is_empty() {
                continue;
            }
            let split = term
                .find(|c: char| c.is_ascii_alphabetic() || c == '"')
                .unwrap_or(term.len());
            let (num, unit) = term.split_at(split);
            let value: f64 = num.trim().parse().ok()?;
            let unit = match unit.trim() {
                "" => self,
                "mm" => Units::Millimeter,
                "cm" => Units::Centimeter,
                "m" => Units::Meter,
                "in" | "\"" => Units::Inch,
                "ft" => Units::Foot,
                _ => return None,
            };
            total += unit.to_mm(value);
            any = true;
        }
        any.then_some(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_expressions() {
        assert_eq!(Units::Millimeter.parse("10"), Some(10.0));
        assert_eq!(Units::Millimeter.parse("1 in"), Some(25.4));
        assert_eq!(Units::Inch.parse("2"), Some(50.8));
        assert_eq!(Units::Millimeter.parse("10mm + 1cm"), Some(20.0));
        assert_eq!(Units::Millimeter.parse("-5"), Some(-5.0));
        assert_eq!(Units::Millimeter.parse("abc"), None);
    }
}
