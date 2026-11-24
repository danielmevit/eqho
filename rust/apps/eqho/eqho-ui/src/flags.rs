//! Vector SVG country/regional flag renderer for all 99 Whisper languages, rasterized
//! via `resvg` at 4× HiDPI (`84×56` RGBA8) for crisp, anti-aliased flags at every DPI scale.
//!
//! Windows 10/11 does not include country flag glyphs in Segoe UI Emoji (rendering monochrome
//! two-letter codes like `US` or `DE` instead). Generating true vector SVG geometry with
//! rounded corners, subtle hairline framing, and `resvg` subpixel anti-aliasing keeps every
//! flag razor-sharp at 100%–200% zoom on both light and dark themes.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Rendered pixel dimensions (4× HiDPI of the `21×14` logical display size).
pub const FLAG_W: u32 = 84;
pub const FLAG_H: u32 = 56;

const WHITE: &str = "#F8FAFC";
const BLACK: &str = "#16181D";
const RED: &str = "#D82A2E";
const DARK_RED: &str = "#A51826";
const CRIMSON: &str = "#DC143C";
const BLUE: &str = "#1656B2";
const NAVY: &str = "#112C66";
const SKY: &str = "#38A5E0";
const GREEN: &str = "#168A42";
const DARK_GREEN: &str = "#0E6034";
const YELLOW: &str = "#F6CA18";
const GOLD: &str = "#F2AE18";
const ORANGE: &str = "#EE7A1C";
const MAROON: &str = "#8A1C34";

#[derive(Clone, Copy)]
enum Pattern {
    H2(&'static str, &'static str),
    H3(&'static str, &'static str, &'static str),
    H3WideMid(&'static str, &'static str, &'static str, &'static str),
    H5(&'static str, &'static str, &'static str, &'static str, &'static str),
    V3(&'static str, &'static str, &'static str),
    V2(&'static str, &'static str),
    Nordic(&'static str, &'static str, &'static str),
    Cross(&'static str, &'static str),
    Disc(&'static str, &'static str),
    StarField(&'static str, &'static str),
    Crescent(&'static str, &'static str),
    Canton(&'static str, &'static str),
    HoistTri2(&'static str, &'static str, &'static str),
    HoistTri3(&'static str, &'static str, &'static str, &'static str),
    H3Badge(&'static str, &'static str, &'static str, &'static str),
    IndiaChakra,
    Diag(&'static str, &'static str, &'static str),
    Saltire(&'static str, &'static str),
    UnionJack,
    UnitedStates,
    Greece,
    Brazil,
    Korea,
    Israel,
    SouthAfrica,
    Malaysia,
    Striped(&'static str, &'static str),
    Wales,
    Basque,
    Bosnia,
    Macedonia,
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
        "vi" => Pattern::StarField(RED, YELLOW),
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
        "hi" => Pattern::IndiaChakra,
        "id" => Pattern::H2(RED, WHITE),
        "ms" => Pattern::Malaysia,
        "th" => Pattern::H5(RED, WHITE, NAVY, WHITE, RED),
        "he" => Pattern::Israel,

        // Experimental (66)
        "af" => Pattern::SouthAfrica,
        "sq" => Pattern::Disc(RED, BLACK),
        "am" => Pattern::H3Badge(GREEN, YELLOW, RED, BLUE),
        "hy" => Pattern::H3(RED, BLUE, ORANGE),
        "as" => Pattern::IndiaChakra,
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
        "gu" => Pattern::IndiaChakra,
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
        "ml" => Pattern::IndiaChakra,
        "mt" => Pattern::V2(WHITE, RED),
        "mi" => Pattern::H3(BLACK, WHITE, RED),
        "mr" => Pattern::IndiaChakra,
        "mn" => Pattern::V3(RED, BLUE, RED),
        "ne" => Pattern::HoistTri2(CRIMSON, CRIMSON, NAVY),
        "nn" => Pattern::Nordic(RED, WHITE, NAVY),
        "oc" => Pattern::Disc(RED, GOLD),
        "ps" => Pattern::V3(BLACK, RED, GREEN),
        "fa" => Pattern::H3Badge(GREEN, WHITE, RED, RED),
        "pa" => Pattern::Pakistan,
        "sa" => Pattern::IndiaChakra,
        "sr" => Pattern::H3Badge(RED, BLUE, WHITE, GOLD),
        "sn" => Pattern::HoistTri3(GREEN, YELLOW, RED, WHITE),
        "sd" => Pattern::Pakistan,
        "si" => Pattern::V2(GOLD, MAROON),
        "sl" => Pattern::H3Badge(WHITE, BLUE, RED, BLUE),
        "so" => Pattern::StarField(SKY, WHITE),
        "su" => Pattern::H2(RED, WHITE),
        "sw" => Pattern::Diag(GREEN, BLACK, SKY),
        "tl" => Pattern::HoistTri2(BLUE, RED, WHITE),
        "tg" => Pattern::H3Badge(RED, WHITE, GREEN, GOLD),
        "ta" => Pattern::IndiaChakra,
        "tt" => Pattern::H3(GREEN, WHITE, RED),
        "te" => Pattern::IndiaChakra,
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

/// Five-pointed star polygon centered at `(cx, cy)` with outer radius `r_out` and inner radius `r_in`.
fn star_polygon(cx: f32, cy: f32, r_out: f32, r_in: f32, fill: &str) -> String {
    let mut pts = String::with_capacity(128);
    for i in 0..10 {
        let angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
        let r = if i % 2 == 0 { r_out } else { r_in };
        let x = cx + r * angle.cos();
        let y = cy + r * angle.sin();
        if i > 0 {
            pts.push(' ');
        }
        use std::fmt::Write;
        let _ = write!(&mut pts, "{x:.1},{y:.1}");
    }
    format!(r#"<polygon points="{pts}" fill="{fill}"/>"#)
}

fn svg_body_for_pattern(pat: Pattern) -> String {
    match pat {
        Pattern::H2(t, b) => format!(
            r#"<rect width="210" height="70" fill="{t}"/><rect y="70" width="210" height="70" fill="{b}"/>"#
        ),
        Pattern::H3(t, m, b) => format!(
            r#"<rect width="210" height="47" fill="{t}"/><rect y="46.5" width="210" height="47" fill="{m}"/><rect y="93" width="210" height="47" fill="{b}"/>"#
        ),
        Pattern::H3WideMid(t, m, b, badge) => {
            let mut s = format!(
                r#"<rect width="210" height="35" fill="{t}"/><rect y="35" width="210" height="70" fill="{m}"/><rect y="105" width="210" height="35" fill="{b}"/>"#
            );
            if badge != m {
                s.push_str(&format!(
                    r#"<rect x="64" y="54" width="26" height="32" rx="6" fill="{badge}"/>"#
                ));
            }
            s
        }
        Pattern::H5(c0, c1, c2, c3, c4) => format!(
            r#"<rect width="210" height="24" fill="{c0}"/><rect y="23" width="210" height="24" fill="{c1}"/><rect y="46" width="210" height="48" fill="{c2}"/><rect y="93" width="210" height="24" fill="{c3}"/><rect y="116" width="210" height="24" fill="{c4}"/>"#
        ),
        Pattern::V3(l, m, r) => format!(
            r#"<rect width="71" height="140" fill="{l}"/><rect x="70" width="71" height="140" fill="{m}"/><rect x="140" width="70" height="140" fill="{r}"/>"#
        ),
        Pattern::V2(l, r) => format!(
            r#"<rect width="84" height="140" fill="{l}"/><rect x="84" width="126" height="140" fill="{r}"/>"#
        ),
        Pattern::Nordic(field, outer, inner) => {
            let mut s = format!(
                r#"<rect width="210" height="140" fill="{field}"/><rect x="60" width="32" height="140" fill="{outer}"/><rect y="54" width="210" height="32" fill="{outer}"/>"#
            );
            if inner != outer {
                s.push_str(&format!(
                    r#"<rect x="68" width="16" height="140" fill="{inner}"/><rect y="62" width="210" height="16" fill="{inner}"/>"#
                ));
            }
            s
        }
        Pattern::Cross(field, cross) => format!(
            r#"<rect width="210" height="140" fill="{field}"/><rect x="91" width="28" height="140" fill="{cross}"/><rect y="56" width="210" height="28" fill="{cross}"/>"#
        ),
        Pattern::Disc(field, circle) => format!(
            r#"<rect width="210" height="140" fill="{field}"/><circle cx="105" cy="70" r="38" fill="{circle}"/>"#
        ),
        Pattern::StarField(field, star) => format!(
            r#"<rect width="210" height="140" fill="{field}"/>{}"#,
            star_polygon(105.0, 70.0, 36.0, 14.0, star)
        ),
        Pattern::Crescent(field, emblem) => format!(
            r#"<rect width="210" height="140" fill="{field}"/><circle cx="88" cy="70" r="34" fill="{emblem}"/><circle cx="99" cy="70" r="27" fill="{field}"/>{}"#,
            star_polygon(136.0, 70.0, 16.0, 6.5, emblem)
        ),
        Pattern::Canton(field, star) => format!(
            r#"<rect width="210" height="140" fill="{field}"/>{}{}{}{}{}"#,
            star_polygon(42.0, 40.0, 20.0, 8.0, star),
            star_polygon(76.0, 22.0, 7.0, 3.0, star),
            star_polygon(88.0, 36.0, 7.0, 3.0, star),
            star_polygon(88.0, 54.0, 7.0, 3.0, star),
            star_polygon(76.0, 68.0, 7.0, 3.0, star),
        ),
        Pattern::HoistTri2(t, b, tri) => format!(
            r#"<rect width="210" height="70" fill="{t}"/><rect y="70" width="210" height="70" fill="{b}"/><polygon points="0,0 100,70 0,140" fill="{tri}"/>"#
        ),
        Pattern::HoistTri3(t, m, b, tri) => format!(
            r#"<rect width="210" height="47" fill="{t}"/><rect y="46.5" width="210" height="47" fill="{m}"/><rect y="93" width="210" height="47" fill="{b}"/><polygon points="0,0 92,70 0,140" fill="{tri}"/>"#
        ),
        Pattern::H3Badge(t, m, b, badge) => format!(
            r#"<rect width="210" height="47" fill="{t}"/><rect y="46.5" width="210" height="47" fill="{m}"/><rect y="93" width="210" height="47" fill="{b}"/><circle cx="105" cy="70" r="16" fill="{badge}"/>"#
        ),
        Pattern::IndiaChakra => format!(
            r#"<rect width="210" height="47" fill="{ORANGE}"/><rect y="46.5" width="210" height="47" fill="{WHITE}"/><rect y="93" width="210" height="47" fill="{GREEN}"/><circle cx="105" cy="70" r="16" fill="none" stroke="{NAVY}" stroke-width="4.5"/><circle cx="105" cy="70" r="4" fill="{NAVY}"/>"#
        ),
        Pattern::Diag(tl, sash, br) => format!(
            r#"<rect width="210" height="140" fill="{br}"/><polygon points="0,0 210,0 0,140" fill="{tl}"/><line x1="0" y1="140" x2="210" y2="0" stroke="{sash}" stroke-width="34"/>"#
        ),
        Pattern::Saltire(field, cross) => format!(
            r#"<rect width="210" height="140" fill="{field}"/><line x1="0" y1="0" x2="210" y2="140" stroke="{cross}" stroke-width="26"/><line x1="210" y1="0" x2="0" y2="140" stroke="{cross}" stroke-width="26"/>"#
        ),
        Pattern::UnionJack => format!(
            r#"<rect width="210" height="140" fill="{NAVY}"/>\
            <line x1="0" y1="0" x2="210" y2="140" stroke="{WHITE}" stroke-width="28"/>\
            <line x1="210" y1="0" x2="0" y2="140" stroke="{WHITE}" stroke-width="28"/>\
            <line x1="0" y1="0" x2="210" y2="140" stroke="{RED}" stroke-width="11"/>\
            <line x1="210" y1="0" x2="0" y2="140" stroke="{RED}" stroke-width="11"/>\
            <rect x="85" width="40" height="140" fill="{WHITE}"/>\
            <rect y="50" width="210" height="40" fill="{WHITE}"/>\
            <rect x="93" width="24" height="140" fill="{RED}"/>\
            <rect y="58" width="210" height="24" fill="{RED}"/>"#
        ),
        Pattern::UnitedStates => {
            let mut s = format!(r#"<rect width="210" height="140" fill="{WHITE}"/>"#);
            for i in 0..7 {
                let y = i as f32 * (140.0 / 13.0) * 2.0;
                let h = 140.0 / 13.0;
                use std::fmt::Write;
                let _ = write!(&mut s, r#"<rect y="{y:.1}" width="210" height="{h:.1}" fill="{RED}"/>"#);
            }
            s.push_str(&format!(r#"<rect width="94" height="75.4" fill="{NAVY}"/>"#));
            for row in 0..4 {
                for col in 0..5 {
                    let cx = 11.0 + col as f32 * 18.0;
                    let cy = 11.0 + row as f32 * 17.5;
                    use std::fmt::Write;
                    let _ = write!(&mut s, r#"<circle cx="{cx:.1}" cy="{cy:.1}" r="3.2" fill="{WHITE}"/>"#);
                }
            }
            s
        }
        Pattern::Greece => {
            let mut s = format!(r#"<rect width="210" height="140" fill="{WHITE}"/>"#);
            let h = 140.0 / 9.0;
            for i in (0..9).step_by(2) {
                let y = i as f32 * h;
                use std::fmt::Write;
                let _ = write!(&mut s, r#"<rect y="{y:.1}" width="210" height="{h:.1}" fill="{BLUE}"/>"#);
            }
            s.push_str(&format!(
                r#"<rect width="84" height="78" fill="{BLUE}"/><rect x="34" width="16" height="78" fill="{WHITE}"/><rect y="31" width="84" height="16" fill="{WHITE}"/>"#
            ));
            s
        }
        Pattern::Brazil => format!(
            r#"<rect width="210" height="140" fill="{GREEN}"/><polygon points="105,16 190,70 105,124 20,70" fill="{YELLOW}"/><circle cx="105" cy="70" r="32" fill="{NAVY}"/><path d="M 75,66 Q 105,56 135,74" fill="none" stroke="{WHITE}" stroke-width="6.5"/>"#
        ),
        Pattern::Korea => format!(
            r#"<rect width="210" height="140" fill="{WHITE}"/>\
            <circle cx="105" cy="70" r="32" fill="{BLUE}"/>\
            <path d="M 73,70 A 32,32 0 0,1 137,70 A 16,16 0 0,1 105,70 A 16,16 0 0,0 73,70 Z" fill="{RED}"/>\
            <g stroke="{BLACK}" stroke-width="5.5" stroke-linecap="butt">\
            <line x1="28" y1="36" x2="52" y2="18"/><line x1="36" y1="45" x2="60" y2="27"/>\
            <line x1="158" y1="18" x2="182" y2="36"/><line x1="150" y1="27" x2="174" y2="45"/>\
            <line x1="28" y1="104" x2="52" y2="122"/><line x1="36" y1="95" x2="60" y2="113"/>\
            <line x1="158" y1="122" x2="182" y2="104"/><line x1="150" y1="113" x2="174" y2="95"/>\
            </g>"#
        ),
        Pattern::Israel => format!(
            r#"<rect width="210" height="140" fill="{WHITE}"/>\
            <rect y="14" width="210" height="18" fill="{BLUE}"/>\
            <rect y="108" width="210" height="18" fill="{BLUE}"/>\
            <polygon points="105,43 128,83 82,83" fill="none" stroke="{BLUE}" stroke-width="5.5"/>\
            <polygon points="105,97 128,57 82,57" fill="none" stroke="{BLUE}" stroke-width="5.5"/>"#
        ),
        Pattern::SouthAfrica => format!(
            r#"<rect width="210" height="70" fill="{RED}"/><rect y="70" width="210" height="70" fill="{NAVY}"/>\
            <path d="M 0,0 L 96,70 L 0,140" fill="none" stroke="{WHITE}" stroke-width="46"/>\
            <line x1="96" y1="70" x2="210" y2="70" stroke="{WHITE}" stroke-width="46"/>\
            <path d="M 0,0 L 96,70 L 0,140" fill="none" stroke="{DARK_GREEN}" stroke-width="30"/>\
            <line x1="96" y1="70" x2="210" y2="70" stroke="{DARK_GREEN}" stroke-width="30"/>\
            <polygon points="0,16 74,70 0,124" fill="{YELLOW}"/><polygon points="0,28 58,70 0,112" fill="{BLACK}"/>"#
        ),
        Pattern::Malaysia => {
            let mut s = format!(r#"<rect width="210" height="140" fill="{WHITE}"/>"#);
            let h = 140.0 / 10.0;
            for i in (0..10).step_by(2) {
                let y = i as f32 * h;
                use std::fmt::Write;
                let _ = write!(&mut s, r#"<rect y="{y:.1}" width="210" height="{h:.1}" fill="{RED}"/>"#);
            }
            s.push_str(&format!(
                r#"<rect width="102" height="78" fill="{NAVY}"/><circle cx="44" cy="39" r="22" fill="{YELLOW}"/><circle cx="51" cy="39" r="18" fill="{NAVY}"/>{}"#,
                star_polygon(74.0, 39.0, 14.0, 6.0, YELLOW)
            ));
            s
        }
        Pattern::Striped(a, b) => {
            let mut s = format!(r#"<rect width="210" height="140" fill="{a}"/>"#);
            let h = 140.0 / 9.0;
            for i in (1..9).step_by(2) {
                let y = i as f32 * h;
                use std::fmt::Write;
                let _ = write!(&mut s, r#"<rect y="{y:.1}" width="210" height="{h:.1}" fill="{b}"/>"#);
            }
            s
        }
        Pattern::Wales => format!(
            r#"<rect width="210" height="70" fill="{WHITE}"/><rect y="70" width="210" height="70" fill="{GREEN}"/><path d="M 54,94 Q 72,44 114,52 Q 146,58 158,42 Q 148,76 130,86 L 146,104 L 116,96 L 94,106 L 84,88 L 62,102 Z" fill="{RED}"/>"#
        ),
        Pattern::Basque => format!(
            r#"<rect width="210" height="140" fill="{RED}"/>\
            <line x1="0" y1="0" x2="210" y2="140" stroke="{DARK_GREEN}" stroke-width="24"/>\
            <line x1="210" y1="0" x2="0" y2="140" stroke="{DARK_GREEN}" stroke-width="24"/>\
            <rect x="93" width="24" height="140" fill="{WHITE}"/>\
            <rect y="58" width="210" height="24" fill="{WHITE}"/>"#
        ),
        Pattern::Bosnia => format!(
            r#"<rect width="210" height="140" fill="{NAVY}"/><polygon points="56,0 166,0 166,140" fill="{YELLOW}"/>{}{}{}{}"#,
            star_polygon(42.0, 20.0, 9.5, 4.0, WHITE),
            star_polygon(64.0, 48.0, 9.5, 4.0, WHITE),
            star_polygon(86.0, 76.0, 9.5, 4.0, WHITE),
            star_polygon(108.0, 104.0, 9.5, 4.0, WHITE),
        ),
        Pattern::Macedonia => format!(
            r#"<rect width="210" height="140" fill="{RED}"/>\
            <g stroke="{YELLOW}" stroke-width="14">\
            <line x1="0" y1="0" x2="210" y2="140"/><line x1="210" y1="0" x2="0" y2="140"/>\
            <line x1="105" y1="0" x2="105" y2="140"/><line x1="0" y1="70" x2="210" y2="70"/>\
            </g>\
            <circle cx="105" cy="70" r="24" fill="{YELLOW}" stroke="{RED}" stroke-width="5"/>"#
        ),
        Pattern::Pakistan => format!(
            r#"<rect width="54" height="140" fill="{WHITE}"/><rect x="54" width="156" height="140" fill="{DARK_GREEN}"/><circle cx="132" cy="70" r="34" fill="{WHITE}"/><circle cx="143" cy="64" r="28" fill="{DARK_GREEN}"/>{}"#,
            star_polygon(154.0, 56.0, 13.0, 5.2, WHITE)
        ),
    }
}

fn flag_svg(code: &str) -> String {
    let body = svg_body_for_pattern(pattern_for_language(code));
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 210 140" width="{FLAG_W}" height="{FLAG_H}">
  <defs>
    <clipPath id="r"><rect width="210" height="140" rx="14" ry="14"/></clipPath>
  </defs>
  <g clip-path="url(#r)">{body}</g>
  <rect x="2.5" y="2.5" width="205" height="135" rx="12" ry="12" fill="none" stroke="#808080" stroke-opacity="0.32" stroke-width="5"/>
</svg>"##
    )
}

fn rasterize_svg_rgba(svg: &str) -> Vec<u8> {
    let opts = resvg::usvg::Options::default();
    let Ok(tree) = resvg::usvg::Tree::from_str(svg, &opts) else {
        return vec![0u8; (FLAG_W * FLAG_H * 4) as usize];
    };
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(FLAG_W, FLAG_H) else {
        return vec![0u8; (FLAG_W * FLAG_H * 4) as usize];
    };
    let sx = FLAG_W as f32 / tree.size().width().max(1.0);
    let sy = FLAG_H as f32 / tree.size().height().max(1.0);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );

    // Convert premultiplied RGBA from tiny-skia into straight RGBA8 for Slint `SharedPixelBuffer`.
    let src = pixmap.data();
    let mut out = vec![0u8; src.len()];
    for (dst_px, src_px) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let a = src_px[3];
        if a == 0 {
            dst_px.copy_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            dst_px.copy_from_slice(src_px);
        } else {
            let af = u32::from(a);
            dst_px[0] = ((u32::from(src_px[0]) * 255 + af / 2) / af).min(255) as u8;
            dst_px[1] = ((u32::from(src_px[1]) * 255 + af / 2) / af).min(255) as u8;
            dst_px[2] = ((u32::from(src_px[2]) * 255 + af / 2) / af).min(255) as u8;
            dst_px[3] = a;
        }
    }
    out
}

fn flag_cache() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_flag_rgba(key: &str) -> Vec<u8> {
    let mut map = flag_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = map.get(key) {
        cached.clone()
    } else {
        let rendered = rasterize_svg_rgba(&flag_svg(key));
        map.insert(key.to_owned(), rendered.clone());
        rendered
    }
}

/// Render a crisp 4× HiDPI (`84×56`) straight RGBA8 vector-SVG country flag for a supported
/// Whisper language code (or `"pt-br"`), returning `None` for empty or unknown codes.
#[must_use]
pub fn render_flag_rgba(code: &str) -> Option<(u32, u32, Vec<u8>)> {
    let key = code.trim().to_ascii_lowercase();
    if key.is_empty()
        || (key != "pt-br" && eqho_core::settings::supported_language_name(&key).is_none())
    {
        return None;
    }
    Some((FLAG_W, FLAG_H, cached_flag_rgba(&key)))
}

/// Render a crisp 4× HiDPI (`84×56`) vector-SVG country flag `slint::Image` for a Whisper language code.
pub fn flag_image_for_language(code: &str) -> Image {
    let key = code.trim().to_ascii_lowercase();
    let rgba = cached_flag_rgba(&key);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, FLAG_W, FLAG_H);
    Image::from_rgba8(buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use eqho_core::settings::LANGUAGE_TIERS;

    #[test]
    fn all_99_languages_render_crisp_hidpi_svg_flags() {
        for entry in LANGUAGE_TIERS {
            let img = flag_image_for_language(entry.code);
            let sz = img.size();
            assert_eq!(sz.width, FLAG_W, "flag width for {}", entry.code);
            assert_eq!(sz.height, FLAG_H, "flag height for {}", entry.code);
            let rgba = rasterize_svg_rgba(&flag_svg(entry.code));
            // Verify center pixel is opaque (flag actually rendered).
            let mid = ((FLAG_H / 2) * FLAG_W + (FLAG_W / 2)) as usize * 4;
            assert_eq!(rgba[mid + 3], 255, "opaque center for {}", entry.code);
        }
    }

    #[test]
    fn render_flag_rgba_validates_normalizes_and_caches() {
        let expected_len = (FLAG_W * FLAG_H * 4) as usize;
        for code in ["en", "BG", "de", "pt-br"] {
            let (w, h, bytes) = render_flag_rgba(code)
                .unwrap_or_else(|| panic!("expected flag bitmap for {code}"));
            assert_eq!(w, FLAG_W);
            assert_eq!(h, FLAG_H);
            assert_eq!(bytes.len(), expected_len);
        }
        assert!(render_flag_rgba("not-a-lang").is_none());
        assert!(render_flag_rgba("").is_none());
        assert!(render_flag_rgba("   ").is_none());
    }
}
