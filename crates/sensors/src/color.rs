//! Copied colors (FR-CLIP-03): the same color as hex, rgb(), hsl() and the
//! nearest Tailwind class, ready to paste.

/// Tailwind's default palette, shades 50 to 950.
const SHADES: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];
const PALETTE: &[(&str, [u32; 11])] = &[
    (
        "slate",
        [
            0xf8fafc, 0xf1f5f9, 0xe2e8f0, 0xcbd5e1, 0x94a3b8, 0x64748b, 0x475569, 0x334155,
            0x1e293b, 0x0f172a, 0x020617,
        ],
    ),
    (
        "gray",
        [
            0xf9fafb, 0xf3f4f6, 0xe5e7eb, 0xd1d5db, 0x9ca3af, 0x6b7280, 0x4b5563, 0x374151,
            0x1f2937, 0x111827, 0x030712,
        ],
    ),
    (
        "zinc",
        [
            0xfafafa, 0xf4f4f5, 0xe4e4e7, 0xd4d4d8, 0xa1a1aa, 0x71717a, 0x52525b, 0x3f3f46,
            0x27272a, 0x18181b, 0x09090b,
        ],
    ),
    (
        "neutral",
        [
            0xfafafa, 0xf5f5f5, 0xe5e5e5, 0xd4d4d4, 0xa3a3a3, 0x737373, 0x525252, 0x404040,
            0x262626, 0x171717, 0x0a0a0a,
        ],
    ),
    (
        "stone",
        [
            0xfafaf9, 0xf5f5f4, 0xe7e5e4, 0xd6d3d1, 0xa8a29e, 0x78716c, 0x57534e, 0x44403c,
            0x292524, 0x1c1917, 0x0c0a09,
        ],
    ),
    (
        "red",
        [
            0xfef2f2, 0xfee2e2, 0xfecaca, 0xfca5a5, 0xf87171, 0xef4444, 0xdc2626, 0xb91c1c,
            0x991b1b, 0x7f1d1d, 0x450a0a,
        ],
    ),
    (
        "orange",
        [
            0xfff7ed, 0xffedd5, 0xfed7aa, 0xfdba74, 0xfb923c, 0xf97316, 0xea580c, 0xc2410c,
            0x9a3412, 0x7c2d12, 0x431407,
        ],
    ),
    (
        "amber",
        [
            0xfffbeb, 0xfef3c7, 0xfde68a, 0xfcd34d, 0xfbbf24, 0xf59e0b, 0xd97706, 0xb45309,
            0x92400e, 0x78350f, 0x451a03,
        ],
    ),
    (
        "yellow",
        [
            0xfefce8, 0xfef9c3, 0xfef08a, 0xfde047, 0xfacc15, 0xeab308, 0xca8a04, 0xa16207,
            0x854d0e, 0x713f12, 0x422006,
        ],
    ),
    (
        "lime",
        [
            0xf7fee7, 0xecfccb, 0xd9f99d, 0xbef264, 0xa3e635, 0x84cc16, 0x65a30d, 0x4d7c0f,
            0x3f6212, 0x365314, 0x1a2e05,
        ],
    ),
    (
        "green",
        [
            0xf0fdf4, 0xdcfce7, 0xbbf7d0, 0x86efac, 0x4ade80, 0x22c55e, 0x16a34a, 0x15803d,
            0x166534, 0x14532d, 0x052e16,
        ],
    ),
    (
        "emerald",
        [
            0xecfdf5, 0xd1fae5, 0xa7f3d0, 0x6ee7b7, 0x34d399, 0x10b981, 0x059669, 0x047857,
            0x065f46, 0x064e3b, 0x022c22,
        ],
    ),
    (
        "teal",
        [
            0xf0fdfa, 0xccfbf1, 0x99f6e4, 0x5eead4, 0x2dd4bf, 0x14b8a6, 0x0d9488, 0x0f766e,
            0x115e59, 0x134e4a, 0x042f2e,
        ],
    ),
    (
        "cyan",
        [
            0xecfeff, 0xcffafe, 0xa5f3fc, 0x67e8f9, 0x22d3ee, 0x06b6d4, 0x0891b2, 0x0e7490,
            0x155e75, 0x164e63, 0x083344,
        ],
    ),
    (
        "sky",
        [
            0xf0f9ff, 0xe0f2fe, 0xbae6fd, 0x7dd3fc, 0x38bdf8, 0x0ea5e9, 0x0284c7, 0x0369a1,
            0x075985, 0x0c4a6e, 0x082f49,
        ],
    ),
    (
        "blue",
        [
            0xeff6ff, 0xdbeafe, 0xbfdbfe, 0x93c5fd, 0x60a5fa, 0x3b82f6, 0x2563eb, 0x1d4ed8,
            0x1e40af, 0x1e3a8a, 0x172554,
        ],
    ),
    (
        "indigo",
        [
            0xeef2ff, 0xe0e7ff, 0xc7d2fe, 0xa5b4fc, 0x818cf8, 0x6366f1, 0x4f46e5, 0x4338ca,
            0x3730a3, 0x312e81, 0x1e1b4b,
        ],
    ),
    (
        "violet",
        [
            0xf5f3ff, 0xede9fe, 0xddd6fe, 0xc4b5fd, 0xa78bfa, 0x8b5cf6, 0x7c3aed, 0x6d28d9,
            0x5b21b6, 0x4c1d95, 0x2e1065,
        ],
    ),
    (
        "purple",
        [
            0xfaf5ff, 0xf3e8ff, 0xe9d5ff, 0xd8b4fe, 0xc084fc, 0xa855f7, 0x9333ea, 0x7e22ce,
            0x6b21a8, 0x581c87, 0x3b0764,
        ],
    ),
    (
        "fuchsia",
        [
            0xfdf4ff, 0xfae8ff, 0xf5d0fe, 0xf0abfc, 0xe879f9, 0xd946ef, 0xc026d3, 0xa21caf,
            0x86198f, 0x701a75, 0x4a044e,
        ],
    ),
    (
        "pink",
        [
            0xfdf2f8, 0xfce7f3, 0xfbcfe8, 0xf9a8d4, 0xf472b6, 0xec4899, 0xdb2777, 0xbe185d,
            0x9d174d, 0x831843, 0x500724,
        ],
    ),
    (
        "rose",
        [
            0xfff1f2, 0xffe4e6, 0xfecdd3, 0xfda4af, 0xfb7185, 0xf43f5e, 0xe11d48, 0xbe123c,
            0x9f1239, 0x881337, 0x4c0519,
        ],
    ),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

fn numbers(inner: &str) -> Vec<f32> {
    inner
        .split([',', ' ', '/'])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.trim_end_matches(['%', 'g', 'd', 'e']).parse::<f32>().ok())
        .collect()
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb {
        r: to(r1),
        g: to(g1),
        b: to(b1),
    }
}

pub fn parse(text: &str) -> Option<Rgb> {
    let t = text.trim().to_ascii_lowercase();
    if let Some(hex) = t.strip_prefix('#') {
        let full: String = match hex.len() {
            3 | 4 => hex.chars().take(3).flat_map(|c| [c, c]).collect(),
            6 | 8 => hex[..6].to_owned(),
            _ => return None,
        };
        let v = u32::from_str_radix(&full, 16).ok()?;
        return Some(Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        });
    }
    let inner = |p: &str| {
        t.strip_prefix(p)
            .and_then(|r| r.strip_prefix('(').or(Some(r)))
            .and_then(|r| r.strip_suffix(')'))
    };
    if let Some(i) = inner("rgba").or_else(|| inner("rgb")) {
        let n = numbers(i);
        if n.len() < 3 {
            return None;
        }
        let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        return Some(Rgb {
            r: c(n[0]),
            g: c(n[1]),
            b: c(n[2]),
        });
    }
    if let Some(i) = inner("hsla").or_else(|| inner("hsl")) {
        let n = numbers(i);
        if n.len() < 3 {
            return None;
        }
        return Some(hsl_to_rgb(n[0], n[1] / 100.0, n[2] / 100.0));
    }
    None
}

fn hsl(c: Rgb) -> (u32, u32, u32) {
    let (r, g, b) = (
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0, 0, (l * 100.0).round() as u32);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (
        h.round() as u32 % 360,
        (s * 100.0).round() as u32,
        (l * 100.0).round() as u32,
    )
}

/// The closest Tailwind color, like `sky-500`.
pub fn nearest_tailwind(c: Rgb) -> String {
    let mut best = (u32::MAX, String::new());
    for (name, shades) in PALETTE {
        for (i, hex) in shades.iter().enumerate() {
            let (r, g, b) = ((hex >> 16) & 0xff, (hex >> 8) & 0xff, hex & 0xff);
            let d = |a: u8, b: u32| (i64::from(a) - i64::from(b)).pow(2) as u32;
            // Weighted toward green, which the eye sees best.
            let dist = 2 * d(c.r, r) + 4 * d(c.g, g) + 3 * d(c.b, b);
            if dist < best.0 {
                best = (dist, format!("{name}-{}", SHADES[i]));
            }
        }
    }
    best.1
}

/// Payload fields for a copied color.
pub fn info(text: &str) -> Option<serde_json::Value> {
    let c = parse(text)?;
    let (h, s, l) = hsl(c);
    Some(serde_json::json!({
        "hex": format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
        "rgb": format!("rgb({} {} {})", c.r, c.g, c.b),
        "hsl": format!("hsl({h} {s}% {l}%)"),
        "tailwind": nearest_tailwind(c),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_formats() {
        let i = info("#0ea5e9").unwrap();
        assert_eq!(i["rgb"], "rgb(14 165 233)");
        assert_eq!(i["tailwind"], "sky-500");
        assert_eq!(i["hsl"], "hsl(199 89% 48%)");
        assert_eq!(info("#fff").unwrap()["hex"], "#ffffff");
        assert_eq!(info("rgb(239, 68, 68)").unwrap()["tailwind"], "red-500");
        assert_eq!(info("hsl(0 0% 100%)").unwrap()["hex"], "#ffffff");
        assert_eq!(info("rgba(0,0,0,0.5)").unwrap()["hex"], "#000000");
        assert!(info("#12").is_none());
        assert!(info("oklch(0.7 0.1 200)").is_none());
    }
}
