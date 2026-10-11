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
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if c == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = if c == b'*' {
                v * r
            } else {
                if r == 0.0 {
                    return Err("division by zero".into());
                }
                v / r
            };
        }
        Ok(v)
    }
    fn power(&mut self) -> Result<f64, String> {
        let b = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let e = self.power()?;
            return Ok(b.powf(e));
        }
        Ok(b)
    }
    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Result<f64, String> {
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
                let save = self.i;
                let unit = self.ident();
                let factor = match unit.as_str() {
                    "" => {
                        self.i = save;
                        self.units.scale()
                    }
                    "mm" => 1.0,
                    "cm" => 10.0,
                    "m" => 1000.0,
                    "in" => 25.4,
                    "ft" => 304.8,
                    "deg" => 1.0,
                    _ => {
                        self.i = save;
                        self.units.scale()
                    }
                };
                Ok(num * factor)
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
                    return match name.as_str() {
                        "sqrt" => Ok(a.sqrt()),
                        "abs" => Ok(a.abs()),
                        "sin" => Ok(a.to_radians().sin()),
                        "cos" => Ok(a.to_radians().cos()),
                        "tan" => Ok(a.to_radians().tan()),
                        "round" => Ok(a.round()),
                        "floor" => Ok(a.floor()),
                        "ceil" => Ok(a.ceil()),
                        _ => Err(format!("unknown function '{name}'")),
                    };
                }
                match name.as_str() {
                    "PI" | "pi" => Ok(std::f64::consts::PI),
                    _ => self.vars.get(&name).copied().ok_or_else(|| format!("unknown parameter '{name}'")),
                }
            }
            _ => Err("expected a value".into()),
        }
    }
}

/// Evaluate an expression. Bare numbers are in `units` (converted to mm).
pub fn eval(text: &str, units: Units, vars: &HashMap<String, f64>) -> Result<f64, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0, units, vars };
    let v = p.expr()?;
    if p.peek().is_some() {
        return Err(format!("unexpected '{}'", &text[p.i..]));
    }
    if !v.is_finite() {
        return Err("result is not a number".into());
    }
    Ok(v)
}

/// Whether an expression references parameters (i.e. is not a plain value).
pub fn is_symbolic(text: &str) -> bool {
    let t = text.trim();
    let stripped = t.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    stripped.chars().any(|c| c.is_ascii_alphabetic() || c == '_') || (t.chars().any(|c| c.is_ascii_alphabetic()) && !["mm", "cm", "m", "in", "ft", "deg"].iter().any(|u| t.ends_with(u)))
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
        assert!(eval("2 +", u, &v).is_err());
    }

    #[test]
    fn symbolic_detection() {
        assert!(!is_symbolic("25"));
        assert!(!is_symbolic("25 mm"));
        assert!(is_symbolic("width/2"));
        assert!(is_symbolic("w + 2 mm"));
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
