//! Pure-Rust procedural 21×14 RGBA8 country/regional flag renderer for all 99
//! Whisper languages.
//!
//! Windows 10/11 does not include country flag glyphs in Segoe UI Emoji (rendering
//! monochrome two-letter codes like `US` or `DE` instead), so generating crisp
//! tiny raster flags guarantees vibrant, uniform flags on every platform.

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

pub const FLAG_W: u32 = 21;
pub const FLAG_H: u32 = 14;

type Rgb = [u8; 3];

const WHITE: Rgb = [248, 250, 252];
const BLACK: Rgb = [22, 24, 29];
const RED: Rgb = [216, 42, 46];
const DARK_RED: Rgb = [165, 24, 38];
const CRIMSON: Rgb = [220, 20, 60];
const BLUE: Rgb = [22, 86, 178];
const NAVY: Rgb = [17, 44, 102];
const SKY: Rgb = [56, 165, 224];
const GREEN: Rgb = [22, 138, 66];
const DARK_GREEN: Rgb = [14, 96, 52];
const YELLOW: Rgb = [246, 202, 24];
const GOLD: Rgb = [242, 174, 24];
const ORANGE: Rgb = [238, 122, 28];
const MAROON: Rgb = [138, 28, 52];

#[derive(Clone, Copy)]
enum Pattern {
    /// Horizontal 2-stripe bicolor (top, bottom)
    H2(Rgb, Rgb),
    /// Horizontal 3-stripe tricolor (top, mid, bottom)
    H3(Rgb, Rgb, Rgb),
    /// Horizontal 3-stripe with Spanish/Austrian wide middle (1:2:1)
    H3WideMid(Rgb, Rgb, Rgb, Rgb),
    /// Horizontal 5-stripe (e.g. Thailand, Costa Rica)
    H5(Rgb, Rgb, Rgb, Rgb, Rgb),
    /// Vertical 3-stripe tricolor (left, mid, right)
    V3(Rgb, Rgb, Rgb),
    /// Vertical 2-stripe bicolor (left, right, optional badge)
    V2(Rgb, Rgb),
    /// Nordic cross (field, outer cross, inner cross)
    Nordic(Rgb, Rgb, Rgb),
    /// Centered symmetric cross (field, cross)
    Cross(Rgb, Rgb),
    /// Solid field with centered circle (field, circle)
    Disc(Rgb, Rgb),
    /// Solid field with left-offset crescent/star badge (field, emblem)
    Crescent(Rgb, Rgb),
    /// Solid field with top-left canton emblem (field, canton_color)
    Canton(Rgb, Rgb),
    /// Horizontal 2-stripe with hoist triangle (top, bottom, triangle)
    HoistTri2(Rgb, Rgb, Rgb),
    /// Horizontal 3-stripe with hoist triangle (top, mid, bottom, triangle)
    HoistTri3(Rgb, Rgb, Rgb, Rgb),
    /// Horizontal 3-stripe with centered badge dot (top, mid, bottom, badge)
    H3Badge(Rgb, Rgb, Rgb, Rgb),
    /// Diagonal sash (top-left, stripe, bottom-right)
    Diag(Rgb, Rgb, Rgb),
    /// Saltire X-cross (field, cross)
    Saltire(Rgb, Rgb),
    /// Union Jack (UK)
    UnionJack,
    /// US Stars & Stripes
    UnitedStates,
    /// Greece (stripes + blue cross canton)
    Greece,
    /// Brazil (green field, yellow diamond, blue globe)
    Brazil,
    /// South Korea (white field, red/blue Taegeuk + corner marks)
    Korea,
    /// Israel (white field, blue horizontal bands + blue star)
    Israel,
    /// South Africa (Y-pall tricolor)
    SouthAfrica,
    /// Malaysia (stripes + navy canton with gold star)
    Malaysia,
    /// Catalan / striped gold-red (Senyera)
    Striped(Rgb, Rgb),
    /// Wales (white/green bicolor + red dragon emblem)
    Wales,
    /// Basque (red field, green saltire, white cross)
    Basque,
    /// Bosnia (blue field, yellow triangle, white edge)
    Bosnia,
    /// Macedonia (red field with yellow sun/rays)
    Macedonia,
    /// Pakistan (white hoist + dark green field + white crescent)
    Pakistan,
}

fn pattern_for_language(code: &str) -> Pattern {
    match code {
        // Tuned (13)
        "en" => Pattern::UnionJack,
        "es" => Pattern::H3WideMid(RED, YELLOW, RED, GOLD),
        "zh" => Pattern::Canton(RED, YELLOW),
        "ja" => Pattern::Disc(WHITE, CRIMSON),
        "ko" => Pattern::Korea,
        "vi" => Pattern::Disc(RED, YELLOW),
        "ar" => Pattern::H3Badge(DARK_GREEN, DARK_GREEN, DARK_GREEN, WHITE),
        "uk" => Pattern::H2(SKY, YELLOW),
        "fr" => Pattern::V3(NAVY, WHITE, RED),
        "de" => Pattern::H3(BLACK, RED, GOLD),
        "pt" => Pattern::V2(DARK_GREEN, RED),
        "ru" => Pattern::H3(WHITE, BLUE, RED),
        "it" => Pattern::V3(GREEN, WHITE, RED),

        // Strong (20)
        "nl" => Pattern::H3(RED, WHITE, BLUE),
        "pl" => Pattern::H2(WHITE, CRIMSON),
        "tr" => Pattern::Crescent(RED, WHITE),
        "sv" => Pattern::Nordic(BLUE, YELLOW, YELLOW),
        "da" => Pattern::Nordic(RED, WHITE, WHITE),
        "no" => Pattern::Nordic(RED, WHITE, NAVY),
        "fi" => Pattern::Nordic(WHITE, BLUE, BLUE),
        "cs" => Pattern::HoistTri2(WHITE, RED, NAVY),
        "ro" => Pattern::V3(NAVY, YELLOW, RED),
        "hu" => Pattern::H3(RED, WHITE, DARK_GREEN),
        "el" => Pattern::Greece,
        "bg" => Pattern::H3(WHITE, GREEN, RED),
        "hr" => Pattern::H3Badge(RED, WHITE, BLUE, RED),
        "sk" => Pattern::H3Badge(WHITE, BLUE, RED, WHITE),
        "ca" => Pattern::Striped(YELLOW, RED),
        "hi" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "id" => Pattern::H2(RED, WHITE),
        "ms" => Pattern::Malaysia,
        "th" => Pattern::H5(RED, WHITE, NAVY, WHITE, RED),
        "he" => Pattern::Israel,

        // Experimental (66)
        "af" => Pattern::SouthAfrica,
        "sq" => Pattern::Disc(RED, BLACK),
        "am" => Pattern::H3Badge(GREEN, YELLOW, RED, BLUE),
        "hy" => Pattern::H3(RED, BLUE, ORANGE),
        "as" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "az" => Pattern::H3Badge(SKY, RED, GREEN, WHITE),
        "ba" => Pattern::H3Badge(BLUE, WHITE, GREEN, GOLD),
        "eu" => Pattern::Basque,
        "be" => Pattern::H2(RED, GREEN),
        "bn" => Pattern::Disc(DARK_GREEN, RED),
        "bs" => Pattern::Bosnia,
        "br" => Pattern::Striped(BLACK, WHITE),
        "my" => Pattern::H3Badge(YELLOW, GREEN, RED, WHITE),
        "et" => Pattern::H3(BLUE, BLACK, WHITE),
        "fo" => Pattern::Nordic(WHITE, BLUE, RED),
        "gl" => Pattern::Diag(WHITE, SKY, WHITE),
        "ka" => Pattern::Cross(WHITE, RED),
        "gu" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "ht" => Pattern::H2(NAVY, RED),
        "ha" => Pattern::V3(GREEN, WHITE, GREEN),
        "haw" => Pattern::UnitedStates,
        "is" => Pattern::Nordic(BLUE, WHITE, RED),
        "jw" => Pattern::H2(RED, WHITE),
        "kn" => Pattern::H2(YELLOW, RED),
        "kk" => Pattern::Disc(SKY, GOLD),
        "km" => Pattern::H3WideMid(NAVY, RED, NAVY, WHITE),
        "lo" => Pattern::H3WideMid(RED, NAVY, RED, WHITE),
        "la" => Pattern::V2(YELLOW, WHITE),
        "lv" => Pattern::H3WideMid(MAROON, WHITE, MAROON, WHITE),
        "ln" => Pattern::Diag(SKY, RED, SKY),
        "lt" => Pattern::H3(GOLD, GREEN, RED),
        "lb" => Pattern::H3(RED, WHITE, SKY),
        "mk" => Pattern::Macedonia,
        "mg" => Pattern::V2(WHITE, RED),
        "ml" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "mt" => Pattern::V2(WHITE, RED),
        "mi" => Pattern::H3(BLACK, WHITE, RED),
        "mr" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "mn" => Pattern::V3(RED, BLUE, RED),
        "ne" => Pattern::HoistTri2(CRIMSON, CRIMSON, NAVY),
        "nn" => Pattern::Nordic(RED, WHITE, NAVY),
        "oc" => Pattern::Disc(RED, GOLD),
        "ps" => Pattern::V3(BLACK, RED, GREEN),
        "fa" => Pattern::H3Badge(GREEN, WHITE, RED, RED),
        "pa" => Pattern::Pakistan,
        "sa" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "sr" => Pattern::H3Badge(RED, BLUE, WHITE, GOLD),
        "sn" => Pattern::HoistTri3(GREEN, YELLOW, RED, WHITE),
        "sd" => Pattern::Pakistan,
        "si" => Pattern::V2(GOLD, MAROON),
        "sl" => Pattern::H3Badge(WHITE, BLUE, RED, BLUE),
        "so" => Pattern::Disc(SKY, WHITE),
        "su" => Pattern::H2(RED, WHITE),
        "sw" => Pattern::Diag(GREEN, BLACK, SKY),
        "tl" => Pattern::HoistTri2(BLUE, RED, WHITE),
        "tg" => Pattern::H3Badge(RED, WHITE, GREEN, GOLD),
        "ta" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "tt" => Pattern::H3(GREEN, WHITE, RED),
        "te" => Pattern::H3Badge(ORANGE, WHITE, GREEN, NAVY),
        "bo" => Pattern::HoistTri2(RED, NAVY, GOLD),
        "tk" => Pattern::V2(DARK_RED, GREEN),
        "ur" => Pattern::Pakistan,
        "uz" => Pattern::H3(SKY, WHITE, GREEN),
        "cy" => Pattern::Wales,
        "yi" => Pattern::Israel,
        "yo" => Pattern::V3(GREEN, WHITE, GREEN),
        "pt-br" => Pattern::Brazil,
        _ => Pattern::Saltire(BLUE, WHITE),
    }
}

fn pixel_for_pattern(pat: Pattern, x: u32, y: u32) -> Rgb {
    let w = FLAG_W;
    let h = FLAG_H;
    match pat {
        Pattern::H2(t, b) => {
            if y < h / 2 { t } else { b }
        }
        Pattern::H3(t, m, b) => {
            if y < 5 {
                t
            } else if y < 9 {
                m
            } else {
                b
            }
        }
        Pattern::H3WideMid(t, m, b, badge) => {
            if (3..=10).contains(&y) {
                let dx = x as i32 - 8;
                let dy = y as i32 - 7;
                if badge != m && dx * dx + dy * dy <= 4 {
                    badge
                } else {
                    m
                }
            } else if y < 3 {
                t
            } else {
                b
            }
        }
        Pattern::H5(c0, c1, c2, c3, c4) => match y {
            0..=1 => c0,
            2..=3 => c1,
            4..=9 => c2,
            10..=11 => c3,
            _ => c4,
        },
        Pattern::V3(l, m, r) => {
            if x < 7 {
                l
            } else if x < 14 {
                m
            } else {
                r
            }
        }
        Pattern::V2(l, r) => {
            if x < 9 { l } else { r }
        }
        Pattern::Nordic(field, outer, inner) => {
            if x == 7 || x == 8 || y == 6 || y == 7 {
                inner
            } else if x == 6 || x == 9 || y == 5 || y == 8 {
                outer
            } else {
                field
            }
        }
        Pattern::Cross(field, cross) => {
            if (9..=11).contains(&x) || (6..=7).contains(&y) {
                cross
            } else {
                field
            }
        }
        Pattern::Disc(field, disc) => {
            let dx = x as i32 - 10;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 11 { disc } else { field }
        }
        Pattern::Crescent(field, emblem) => {
            let dx = x as i32 - 8;
            let dy = y as i32 - 7;
            let dx2 = x as i32 - 10;
            let in_outer = dx * dx + dy * dy <= 12;
            let in_cut = dx2 * dx2 + dy * dy <= 9;
            let in_star = (12..=14).contains(&x) && (6..=8).contains(&y);
            if (in_outer && !in_cut) || in_star {
                emblem
            } else {
                field
            }
        }
        Pattern::Canton(field, canton) => {
            let main_star = (2..=5).contains(&x) && (2..=5).contains(&y);
            let small_stars = (x == 7 && (y == 2 || y == 5)) || (x == 8 && (y == 3 || y == 4));
            if main_star || small_stars {
                canton
            } else {
                field
            }
        }
        Pattern::HoistTri2(t, b, tri) => {
            let dist = (y as i32 - 6).abs().min((y as i32 - 7).abs()) as u32;
            if x + dist < 9 {
                tri
            } else if y < h / 2 {
                t
            } else {
                b
            }
        }
        Pattern::HoistTri3(t, m, b, tri) => {
            let dist = (y as i32 - 6).abs().min((y as i32 - 7).abs()) as u32;
            if x + dist < 9 {
                tri
            } else if y < 5 {
                t
            } else if y < 9 {
                m
            } else {
                b
            }
        }
        Pattern::H3Badge(t, m, b, badge) => {
            let dx = x as i32 - 10;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 3 {
                badge
            } else if y < 5 {
                t
            } else if y < 9 {
                m
            } else {
                b
            }
        }
        Pattern::Diag(tl, mid, br) => {
            let v = (x as i32) * 2 + (y as i32) * 3;
            if (34..=48).contains(&v) {
                mid
            } else if v < 34 {
                tl
            } else {
                br
            }
        }
        Pattern::Saltire(field, cross) => {
            let d1 = ((x as i32 * h as i32) - (y as i32 * w as i32)).abs();
            let d2 = ((x as i32 * h as i32) - ((h as i32 - 1 - y as i32) * w as i32)).abs();
            if d1 <= 24 || d2 <= 24 { cross } else { field }
        }
        Pattern::UnionJack => {
            let is_red_cross = (9..=11).contains(&x) || (6..=7).contains(&y);
            let is_white_cross = (8..=12).contains(&x) || (5..=8).contains(&y);
            let d1 = ((x as i32 * h as i32) - (y as i32 * w as i32)).abs();
            let d2 = ((x as i32 * h as i32) - ((h as i32 - 1 - y as i32) * w as i32)).abs();
            if is_red_cross {
                RED
            } else if is_white_cross {
                WHITE
            } else if d1 <= 14 || d2 <= 14 {
                RED
            } else if d1 <= 28 || d2 <= 28 {
                WHITE
            } else {
                NAVY
            }
        }
        Pattern::UnitedStates => {
            if x < 9 && y < 7 {
                if (x + y).is_multiple_of(2) && x > 0 && y > 0 && x < 8 && y < 6 {
                    WHITE
                } else {
                    NAVY
                }
            } else if y.is_multiple_of(2) {
                RED
            } else {
                WHITE
            }
        }
        Pattern::Greece => {
            if x < 8 && y < 8 {
                if x == 3 || x == 4 || y == 3 || y == 4 {
                    WHITE
                } else {
                    BLUE
                }
            } else if (y / 2).is_multiple_of(2) {
                BLUE
            } else {
                WHITE
            }
        }
        Pattern::Brazil => {
            let dx = (x as i32 - 10).abs();
            let dy = (y as i32 - 7).abs();
            if dx * dx + dy * dy <= 7 {
                NAVY
            } else if dx * 6 + dy * 10 <= 48 {
                YELLOW
            } else {
                GREEN
            }
        }
        Pattern::Korea => {
            let dx = x as i32 - 10;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 11 {
                if dy < 0 { RED } else { BLUE }
            } else if (x == 3 || x == 4 || x == 16 || x == 17)
                && (y == 2 || y == 3 || y == 10 || y == 11)
            {
                BLACK
            } else {
                WHITE
            }
        }
        Pattern::Israel => {
            let is_band = y == 2 || y == 3 || y == 10 || y == 11;
            let is_star =
                (8..=12).contains(&x) && (5..=8).contains(&y) && (x + y).is_multiple_of(2);
            if is_band || is_star { BLUE } else { WHITE }
        }
        Pattern::SouthAfrica => {
            let dist = (y as i32 - 6).abs().min((y as i32 - 7).abs()) as u32;
            if x + dist < 6 {
                BLACK
            } else if x + dist == 6 {
                YELLOW
            } else if x + dist <= 9 || (5..=8).contains(&y) {
                GREEN
            } else if y == 4 || y == 9 {
                WHITE
            } else if y < 4 {
                RED
            } else {
                NAVY
            }
        }
        Pattern::Malaysia => {
            if x < 9 && y < 7 {
                if (3..=5).contains(&x) && (2..=4).contains(&y) {
                    YELLOW
                } else {
                    NAVY
                }
            } else if y.is_multiple_of(2) {
                RED
            } else {
                WHITE
            }
        }
        Pattern::Striped(a, b) => {
            if (y / 2).is_multiple_of(2) { a } else { b }
        }
        Pattern::Wales => {
            let dx = x as i32 - 10;
            let dy = y as i32 - 7;
            if dx.abs() <= 4 && dy.abs() <= 2 {
                RED
            } else if y < h / 2 {
                WHITE
            } else {
                GREEN
            }
        }
        Pattern::Basque => {
            if (9..=11).contains(&x) || (6..=7).contains(&y) {
                WHITE
            } else {
                let d1 = ((x as i32 * h as i32) - (y as i32 * w as i32)).abs();
                let d2 = ((x as i32 * h as i32) - ((h as i32 - 1 - y as i32) * w as i32)).abs();
                if d1 <= 22 || d2 <= 22 { GREEN } else { RED }
            }
        }
        Pattern::Bosnia => {
            if (5..=14).contains(&x) && (y as i32) <= (x as i32 - 5) * 14 / 10 {
                YELLOW
            } else {
                NAVY
            }
        }
        Pattern::Macedonia => {
            let dx = x as i32 - 10;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 7 || x == 10 || y == 7 {
                YELLOW
            } else {
                let d1 = ((x as i32 * h as i32) - (y as i32 * w as i32)).abs();
                let d2 = ((x as i32 * h as i32) - ((h as i32 - 1 - y as i32) * w as i32)).abs();
                if d1 <= 16 || d2 <= 16 { YELLOW } else { RED }
            }
        }
        Pattern::Pakistan => {
            if x < 5 {
                WHITE
            } else {
                let dx = x as i32 - 12;
                let dy = y as i32 - 7;
                let dx2 = x as i32 - 14;
                let in_c = dx * dx + dy * dy <= 10 && dx2 * dx2 + dy * dy > 7;
                if in_c || (x == 15 && y == 6) {
                    WHITE
                } else {
                    DARK_GREEN
                }
            }
        }
    }
}

/// Render a crisp 21×14 RGBA8 flag with rounded 1 px corner trim and subtle inner hairline
/// so white flag edges stay clearly defined on both light and dark UI surfaces.
#[must_use]
pub fn flag_rgba_for_language(code: &str) -> (u32, u32, Vec<u8>) {
    let pat = pattern_for_language(code);
    let mut out = vec![0u8; (FLAG_W * FLAG_H * 4) as usize];

    for y in 0..FLAG_H {
        for x in 0..FLAG_W {
            let idx = ((y * FLAG_W + x) * 4) as usize;
            // Subtle 1 px rounded corners
            let is_corner = (x == 0 || x == FLAG_W - 1) && (y == 0 || y == FLAG_H - 1);
            if is_corner {
                out[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let [mut r, mut g, mut b] = pixel_for_pattern(pat, x, y);
            // Subtle 12% neutral inner edge border so light/dark surfaces never swallow flag edges
            let is_edge = x == 0 || x == FLAG_W - 1 || y == 0 || y == FLAG_H - 1;
            if is_edge {
                r = ((u16::from(r) * 86 + 128 * 14) / 100) as u8;
                g = ((u16::from(g) * 86 + 128 * 14) / 100) as u8;
                b = ((u16::from(b) * 86 + 128 * 14) / 100) as u8;
            }
            out[idx] = r;
            out[idx + 1] = g;
            out[idx + 2] = b;
            out[idx + 3] = 255;
        }
    }
    (FLAG_W, FLAG_H, out)
}

/// Convert a language code into a ready-to-render `slint::Image` country flag.
#[must_use]
pub fn flag_image_for_language(code: &str) -> Image {
    let (w, h, rgba) = flag_rgba_for_language(code);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Image::from_rgba8(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use eqho_core::settings::LANGUAGE_TIERS;

    #[test]
    fn renders_valid_non_uniform_flags_for_all_99_languages() {
        for entry in LANGUAGE_TIERS {
            let (w, h, rgba) = flag_rgba_for_language(entry.code);
            assert_eq!((w, h), (FLAG_W, FLAG_H));
            assert_eq!(rgba.len(), (FLAG_W * FLAG_H * 4) as usize);
            // Center pixel must be opaque
            let center = (((FLAG_H / 2) * FLAG_W + (FLAG_W / 2)) * 4) as usize;
            assert_eq!(rgba[center + 3], 255, "flag {} center not opaque", entry.code);
            // Flag must have color variation (not a single blank rectangle)
            let first_inner = &rgba[((FLAG_W + 1) * 4) as usize..((FLAG_W + 1) * 4 + 4) as usize];
            let has_var = rgba
                .chunks_exact(4)
                .any(|px| px[3] == 255 && px != first_inner);
            assert!(has_var, "flag {} rendered uniform", entry.code);
        }
        // Distinct major flags differ
        assert_ne!(flag_rgba_for_language("en").2, flag_rgba_for_language("de").2);
        assert_ne!(flag_rgba_for_language("fr").2, flag_rgba_for_language("it").2);
        assert_ne!(flag_rgba_for_language("ja").2, flag_rgba_for_language("ko").2);
    }
}
