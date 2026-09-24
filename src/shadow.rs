//! The shadow renderer: a 32x32 character display with a 16-colour palette
//! driven by the two memory-mapped planes. It renders to ANSI in a terminal
//! and to a plain text frame for off-line capture.

use crate::cpu::{CHAR_BASE, COLOR_BASE, PLANE_SIZE, ScreenPlane};

pub const W: usize = 32;
pub const H: usize = 32;

/// The 16-entry palette as ANSI foreground codes. Indices 0-7 are the base
/// colours, 8-15 the bright variants.
const PALETTE_ANSI: [&str; 16] = [
    "30", "31", "32", "33", "34", "35", "36", "37", "90", "91", "92", "93", "94", "95", "96",
    "97",
];

/// ANSI foreground colour names used by the text dump (for PNG capture).
pub const PALETTE_NAMES: [&str; 16] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "gray", "brightred",
    "brightgreen", "brightyellow", "brightblue", "brightmagenta", "brightcyan", "brightwhite",
];

/// Returns the visible character for a cell byte: 0 is a space; control
/// codes map to a light dot so the grid stays readable.
fn glyph(b: u8) -> char {
    match b {
        0 => ' ',
        32..=126 => b as char,
        _ => '.',
    }
}

/// Builds an ANSI-rendered screen (bordered, coloured).
pub fn render_ansi(plane: &ScreenPlane) -> String {
    let mut sb = String::with_capacity(6 * PLANE_SIZE);
    sb.push_str("\x1b[H"); // home
    sb.push_str("\x1b[2J"); // clear
    sb.push_str("┌");
    sb.push_str(&"─".repeat(W * 2));
    sb.push_str("┐\n");
    for y in 0..H {
        sb.push_str("│ ");
        for x in 0..W {
            let i = y * W + x;
            let color = (plane.colors[i] & 0x0F) as usize % PALETTE_ANSI.len();
            let ch = glyph(plane.chars[i]);
            sb.push_str("\x1b[");
            sb.push_str(PALETTE_ANSI[color]);
            sb.push_str("m");
            sb.push(ch);
            sb.push_str("\x1b[0m");
        }
        sb.push_str(" │\n");
    }
    sb.push_str("└");
    sb.push_str(&"─".repeat(W * 2));
    sb.push_str("┘\n");
    sb
}

/// Renders a plain-text frame where each line describes one cell as
/// `Xc` (char, colour name). Deterministic and easy to post-process into
/// images or data.
pub fn frame_text(plane: &ScreenPlane) -> Vec<String> {
    let mut out = Vec::with_capacity(H);
    for y in 0..H {
        let mut line = String::with_capacity(W * 8);
        for x in 0..W {
            let i = y * W + x;
            let c = plane.chars[i] as char;
            let color = (plane.colors[i] & 0x0F) as usize % PALETTE_NAMES.len();
            line.push_str(&format!("{c}{},", PALETTE_NAMES[color]));
        }
        line.pop(); // trailing comma
        out.push(line);
    }
    out
}

/// Addresses of the two planes, exposed for host-side tooling.
pub fn plane_addresses() -> (u16, u16) {
    (CHAR_BASE, COLOR_BASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> ScreenPlane {
        ScreenPlane { chars: [0; PLANE_SIZE], colors: [0; PLANE_SIZE] }
    }

    #[test]
    fn glyph_maps_specials() {
        assert_eq!(glyph(0), ' ');
        assert_eq!(glyph(b'A'), 'A');
        assert_eq!(glyph(2), '.');
    }

    #[test]
    fn ansi_render_has_border() {
        let s = render_ansi(&blank());
        assert!(s.contains('┌') && s.contains('└'));
        assert!(s.contains('│'));
    }

    #[test]
    fn frame_text_is_deterministic() {
        let mut p = blank();
        p.chars[0] = b'A';
        p.colors[0] = 2;
        let f1 = frame_text(&p);
        let f2 = frame_text(&p);
        assert_eq!(f1, f2);
        assert_eq!(f1[0].starts_with('A'), true);
        assert!(f1[0].contains("green"));
    }

    #[test]
    fn frame_has_32_lines_and_32_cells() {
        let f = frame_text(&blank());
        assert_eq!(f.len(), 32);
        assert_eq!(f[0].split(',').count(), 32);
    }
}