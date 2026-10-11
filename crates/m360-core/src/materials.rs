//! Physical materials (density and a matching display color).

/// A physical material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    pub name: &'static str,
    /// Density in g/cm³.
    pub density: f64,
    pub color: [f32; 3],
    /// Young's modulus (MPa = N/mm²).
    pub youngs: f64,
    pub poisson: f64,
    /// Yield (or tensile) strength (MPa).
    pub yield_strength: f64,
}

const fn m(name: &'static str, density: f64, color: [f32; 3], youngs: f64, poisson: f64, yield_strength: f64) -> Material {
    Material { name, density, color, youngs, poisson, yield_strength }
}

/// The material library (Steel is the default, as in Fusion).
pub const MATERIALS: &[Material] = &[
    m("Steel", 7.85, [0.62, 0.64, 0.67], 210000.0, 0.3, 250.0),
    m("Stainless Steel", 8.0, [0.75, 0.76, 0.78], 193000.0, 0.29, 215.0),
    m("Aluminum", 2.70, [0.80, 0.82, 0.85], 69000.0, 0.33, 276.0),
    m("Brass", 8.50, [0.80, 0.65, 0.30], 100000.0, 0.34, 200.0),
    m("Copper", 8.96, [0.78, 0.45, 0.28], 117000.0, 0.34, 70.0),
    m("Titanium", 4.43, [0.55, 0.55, 0.58], 114000.0, 0.34, 880.0),
    m("Cast Iron", 7.20, [0.35, 0.35, 0.37], 110000.0, 0.26, 200.0),
    m("Gold", 19.30, [0.90, 0.72, 0.25], 79000.0, 0.42, 100.0),
    m("ABS Plastic", 1.05, [0.92, 0.92, 0.90], 2200.0, 0.35, 40.0),
    m("PLA", 1.24, [0.95, 0.55, 0.20], 3500.0, 0.36, 50.0),
    m("PETG", 1.27, [0.30, 0.55, 0.85], 2100.0, 0.38, 50.0),
    m("Nylon", 1.14, [0.93, 0.92, 0.86], 2700.0, 0.39, 45.0),
    m("Polycarbonate", 1.20, [0.85, 0.88, 0.92], 2400.0, 0.37, 62.0),
    m("Acrylic", 1.18, [0.80, 0.90, 0.95], 3200.0, 0.37, 70.0),
    m("Rubber", 1.20, [0.15, 0.15, 0.16], 10.0, 0.49, 15.0),
    m("Oak", 0.75, [0.70, 0.52, 0.33], 11000.0, 0.3, 40.0),
    m("Pine", 0.50, [0.86, 0.72, 0.50], 9000.0, 0.3, 35.0),
    m("Glass", 2.50, [0.75, 0.88, 0.90], 70000.0, 0.22, 50.0),
    m("Concrete", 2.40, [0.62, 0.62, 0.60], 30000.0, 0.2, 3.0),
];

/// Material by name (Steel when unknown or unset).
pub fn material(name: Option<&str>) -> &'static Material {
    name.and_then(|n| MATERIALS.iter().find(|m| m.name == n)).unwrap_or(&MATERIALS[0])
}

/// Mass in grams of `volume_mm3` of a material.
pub fn mass_g(volume_mm3: f64, m: &Material) -> f64 {
    volume_mm3 / 1000.0 * m.density
}
