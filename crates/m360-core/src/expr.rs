//! Parameter expressions: `+ - * / ^ ( )`, numbers with optional units
//! (mm, cm, m, in, ft, deg), user parameter names and a few functions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Units;

/// A named user parameter (Fusion's "User Parameters").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserParam {
    pub name: String,
    pub expr: String,
    #[serde(default)]
    pub comment: String,
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    units: Units,
    vars: &'a HashMap<String, f64>,
}

/// A value during evaluation: a length (mm, from an explicit unit or a parameter) or a
/// bare number. Bare numbers scale lengths (`w / 2`), are lengths in the document unit
/// when added to one (`1 in + 1`), and become document-unit lengths if the whole
/// expression is unitless (`2 * 3`).
#[derive(Clone, Copy)]
enum Q {
    Len(f64),
    Num(f64),
}

impl Q {
    fn map(self, f: impl Fn(f64) -> f64) -> Q {
        match self {
            Q::Len(v) => Q::Len(f(v)),
            Q::Num(v) => Q::Num(f(v)),
        }
    }
    fn raw(self) -> f64 {
        match self {
            Q::Len(v) | Q::Num(v) => v,
        }
    }
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn ident(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }
    fn len(&self, q: Q) -> f64 {
        match q {
            Q::Len(v) => v,
            Q::Num(v) => v * self.units.scale(),
        }
    }
    fn expr(&mut self) -> Result<Q, String> {
        let mut v = self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            let sign = if c == b'+' { 1.0 } else { -1.0 };
            v = match (v, r) {
                (Q::Num(a), Q::Num(b)) => Q::Num(a + sign * b),
                _ => Q::Len(self.len(v) + sign * self.len(r)),
            };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<Q, String> {
        let mut v = self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            v = if c == b'*' {
                match (v, r) {
                    (Q::Num(a), Q::Num(b)) => Q::Num(a * b),
                    _ => Q::Len(v.raw() * r.raw()),
                }
            } else {
                if r.raw() == 0.0 {
                    return Err("division by zero".into());
                }
                match (v, r) {
                    (Q::Len(a), Q::Len(b)) | (Q::Num(a), Q::Num(b)) => Q::Num(a / b),
                    _ => Q::Len(v.raw() / r.raw()),
                }
            };
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<Q, String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(self.unary()?.map(|v| -v))
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }
    fn power(&mut self) -> Result<Q, String> {
        let b = self.atom()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            // Right-associative; the exponent may be signed (2^-1).
            let e = self.unary()?.raw();
            return Ok(b.map(|v| v.powf(e)));
        }
        Ok(b)
    }
    fn atom(&mut self) -> Result<Q, String> {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(b')') {
                    return Err("missing ')'".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                // Exponent (1e-3)
                if self.i < self.s.len() && (self.s[self.i] == b'e' || self.s[self.i] == b'E') {
                    let save = self.i;
                    self.i += 1;
                    if self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i] == b'+') {
                        self.i += 1;
                    }
                    if self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                            self.i += 1;
                        }
                    } else {
                        self.i = save;
                    }
                }
                let num: f64 = std::str::from_utf8(&self.s[start..self.i]).unwrap().parse().map_err(|_| "invalid number".to_string())?;
                // Optional unit directly after the number.
                self.ws();
                if self.s.get(self.i) == Some(&b'"') {
                    self.i += 1;
                    return Ok(Q::Len(num * 25.4));
                }
                let save = self.i;
                let unit = self.ident();
                let factor = match unit.as_str() {
                    "mm" => 1.0,
                    "cm" => 10.0,
                    "m" => 1000.0,
                    "in" => 25.4,
                    "ft" => 304.8,
                    "deg" => 1.0,
                    _ => {
                        self.i = save;
                        return Ok(Q::Num(num));
                    }
                };
                Ok(Q::Len(num * factor))
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let name = self.ident();
                if self.peek() == Some(b'(') {
                    self.i += 1;
                    let a = self.expr()?;
                    if self.peek() != Some(b')') {
                        return Err("missing ')'".into());
                    }
                    self.i += 1;
                    // Rounding works in the document unit.
                    let s = self.units.scale();
                    let round = |f: fn(f64) -> f64| match a {
                        Q::Len(v) => Q::Len(f(v / s) * s),
                        Q::Num(v) => Q::Num(f(v)),
                    };
                    let trig = |f: fn(f64) -> f64| Q::Num(f(a.raw().to_radians()));
                    return match name.as_str() {
                        "sqrt" => Ok(a.map(f64::sqrt)),
                        "abs" => Ok(a.map(f64::abs)),
                        "sin" => Ok(trig(f64::sin)),
                        "cos" => Ok(trig(f64::cos)),
                        "tan" => Ok(trig(f64::tan)),
                        "round" => Ok(round(f64::round)),
                        "floor" => Ok(round(f64::floor)),
                        "ceil" => Ok(round(f64::ceil)),
                        _ => Err(format!("unknown function '{name}'")),
                    };
                }
                match name.as_str() {
                    "PI" | "pi" => Ok(Q::Num(std::f64::consts::PI)),
                    _ => self.vars.get(&name).map(|v| Q::Len(*v)).ok_or_else(|| format!("unknown parameter '{name}'")),
                }
            }
            _ => Err("expected a value".into()),
        }
    }
}

/// Evaluate an expression. Unitless results are in `units` (converted to mm).
pub fn eval(text: &str, units: Units, vars: &HashMap<String, f64>) -> Result<f64, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0, units, vars };
    let q = p.expr()?;
    if p.peek().is_some() {
        return Err(format!("unexpected '{}'", &text[p.i..]));
    }
    let v = p.len(q);
    if !v.is_finite() {
        return Err("result is not a number".into());
    }
    Ok(v)
}

/// Whether an expression references parameters (i.e. is not a plain value).
pub fn is_symbolic(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    // Last non-blank character before the current token.
    let mut prev = b' ';
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            let after_number = prev.is_ascii_digit() || prev == b'.';
            let unit = after_number && ["mm", "cm", "m", "in", "ft", "deg"].contains(&word);
            let exponent = after_number && (word == "e" || word == "E");
            let next = text[i..].trim_start().bytes().next();
            let function = next == Some(b'(');
            if !unit && !exponent && !function && word != "pi" && word != "PI" {
                return true;
            }
            prev = b[i - 1];
            continue;
        }
        if !c.is_ascii_whitespace() {
            prev = c;
        }
        i += 1;
    }
    false
}

/// Evaluate all user parameters in order (later ones may use earlier ones).
pub fn eval_params(params: &[UserParam], units: Units) -> (HashMap<String, f64>, Vec<(String, String)>) {
    let mut vars = HashMap::new();
    let mut errors = Vec::new();
    for p in params {
        match eval(&p.expr, units, &vars) {
            Ok(v) => {
                vars.insert(p.name.clone(), v);
            }
            Err(e) => errors.push((p.name.clone(), e)),
        }
    }
    (vars, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_units_and_vars() {
        let mut v = HashMap::new();
        v.insert("width".to_string(), 40.0);
        let u = Units::Millimeter;
        assert_eq!(eval("width / 2 + 3 mm", u, &v).unwrap(), 23.0);
        assert_eq!(eval("1 in + 1", u, &v).unwrap(), 26.4);
        assert_eq!(eval("2 * (3 + 4)", u, &v).unwrap(), 14.0);
        assert_eq!(eval("2^3", u, &v).unwrap(), 8.0);
        assert!((eval("sqrt(16) + cos(0)", u, &v).unwrap() - 5.0).abs() < 1e-12);
        assert_eq!(eval("-width", u, &v).unwrap(), -40.0);
        assert_eq!(eval("1.5", Units::Centimeter, &v).unwrap(), 15.0);
        assert!(eval("height", u, &v).is_err());
        assert_eq!(eval("-2^2", u, &v).unwrap(), -4.0);
        assert_eq!(eval("2^-1", u, &v).unwrap(), 0.5);
        assert_eq!(eval("10 - 5", u, &v).unwrap(), 5.0);
        assert_eq!(eval("1e-3", u, &v).unwrap(), 0.001);
        assert_eq!(eval("1\"", u, &v).unwrap(), 25.4);
        assert!(eval("2 +", u, &v).is_err());
    }

    #[test]
    fn symbolic_detection() {
        assert!(!is_symbolic("25"));
        assert!(!is_symbolic("25 mm"));
        assert!(is_symbolic("width/2"));
        assert!(is_symbolic("w + 2 mm"));
        assert!(is_symbolic("2*margin"));
        assert!(is_symbolic("diam"));
        assert!(is_symbolic("shaft + 1in"));
        assert!(!is_symbolic("1.5e-3 m + 2in"));
        assert!(!is_symbolic("sqrt(16) * pi"));
    }

    #[test]
    fn bare_numbers_scale_in_other_units() {
        let mut v = HashMap::new();
        v.insert("w".to_string(), 200.0); // 20 cm
        let u = Units::Centimeter;
        assert_eq!(eval("w / 2", u, &v).unwrap(), 100.0);
        assert_eq!(eval("2 * w", u, &v).unwrap(), 400.0);
        assert_eq!(eval("w + 1", u, &v).unwrap(), 210.0);
        assert_eq!(eval("2 * 3", u, &v).unwrap(), 60.0);
        assert_eq!(eval("w / 10 mm", u, &v).unwrap(), 200.0);
        assert!((eval("sqrt(16)", Units::Inch, &v).unwrap() - 4.0 * 25.4).abs() < 1e-9);
        let ps = vec![
            UserParam { name: "w".into(), expr: "20".into(), comment: String::new() },
            UserParam { name: "h".into(), expr: "w/2".into(), comment: String::new() },
        ];
        let (vars, _) = eval_params(&ps, u);
        assert_eq!(vars["h"], 100.0);
    }

    #[test]
    fn chained_params() {
        let ps = vec![
            UserParam { name: "a".into(), expr: "10".into(), comment: String::new() },
            UserParam { name: "b".into(), expr: "a * 3".into(), comment: String::new() },
            UserParam { name: "c".into(), expr: "zz".into(), comment: String::new() },
        ];
        let (vars, errs) = eval_params(&ps, Units::Millimeter);
        assert_eq!(vars["b"], 30.0);
        assert_eq!(errs.len(), 1);
    }
}
