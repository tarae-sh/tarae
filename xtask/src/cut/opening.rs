//! The opening's card of what's built (`Card::Built`), drawn: the name large in the text's colour (the accent stays the
//! brand's), its line under it, up to two lines drawn as code — a shell's, as you'd type them —, and the small line the
//! cut makes (`told`), faint. Placed as the title card is: one centred column on the ground, nothing else (no seal: the
//! intro just showed the brand).

use super::Opening;
use crate::recording::Picture;
use crate::scenes::docshots::Bg;
use crate::shot::{self, Fonts, Rgb};
use crate::video::{self, Look, Scale};

/// The name: larger than a title card's, it's what the video is about.
pub(crate) const NAME_PX: f32 = 96.0;
/// A line of code: a caption's size.
const CODE_PX: f32 = video::CAPTION_PX;
/// The small line: the band's words' size.
const TOLD_PX: f32 = super::draw::MARK_PX;
/// The air over the code and over the small line (px at 1080p); the name and its line sit close, as a title and its
/// subtitle.
const OVER_CODE: usize = 56;
const OVER_TOLD: usize = 72;

/// What code is drawn in on `bg` (meok's and hanji's palettes, `src/themes/`): the prompt and the small line (their
/// comment), the command (their function), a quoted string (their string); the rest the text's colour.
struct Colours {
    faint: Rgb,
    command: Rgb,
    string: Rgb,
}

fn colours(bg: Bg) -> Colours {
    match bg {
        Bg::Dark => {
            Colours { faint: [0xa3, 0x97, 0x82], command: [0xe6, 0xbd, 0x66], string: [0xab, 0xd0, 0x6a] }
        }
        Bg::Light => {
            Colours { faint: [0x5e, 0x54, 0x46], command: [0x7a, 0x64, 0x00], string: [0x40, 0x6c, 0x00] }
        }
    }
}

/// A line of code in its colours: `$ ` faint, the first word the command's, a quoted string the string's (its quotes
/// too), the rest `ink`.
pub(crate) fn spans(line: &str, ink: Rgb, bg: Bg) -> Vec<(String, Rgb)> {
    let c = colours(bg);
    let mut out: Vec<(String, Rgb)> = vec![("$ ".into(), c.faint)];
    let (mut quoted, mut first) = (false, true);
    for ch in line.chars() {
        let fg = match ch {
            '"' => {
                quoted = !quoted;
                c.string
            }
            _ if quoted => c.string,
            ' ' => {
                first = false;
                ink
            }
            _ if first => c.command,
            _ => ink,
        };
        match out.last_mut() {
            Some(l) if l.1 == fg => l.0.push(ch),
            _ => out.push((ch.to_string(), fg)),
        }
    }
    out
}

/// `lines` (each its `spans`) as rows of cells at `px` on `ground`, one under the next as a terminal has them, by the
/// shots' renderer at `s`.
fn rows(lines: &[Vec<(String, Rgb)>], px: f32, ground: Rgb, fonts: &Fonts, s: Scale) -> Picture {
    let width = |l: &Vec<(String, Rgb)>| l.iter().map(|s| s.0.chars().count()).sum::<usize>();
    let cols = lines.iter().map(width).max().unwrap_or(1).max(1) as u16;
    let mut p = vt100::Parser::new(lines.len().max(1) as u16, cols, 0);
    let [gr, gg, gb] = ground;
    let mut bytes = format!("\x1b[?25l\x1b[48;2;{gr};{gg};{gb}m\x1b[2J");
    for (k, spans) in lines.iter().enumerate() {
        bytes += &format!("\x1b[{};1H", k + 1);
        for (t, [r, g, b]) in spans {
            bytes += &format!("\x1b[38;2;{r};{g};{b};48;2;{gr};{gg};{gb}m{t}");
        }
    }
    p.process(bytes.as_bytes());
    shot::render_scaled(p.screen(), fonts, px, s.k())
}

/// The card's frame at `s`: `o`'s words and the small line `told`, one centred column — the code's lines one block,
/// as a terminal's rows, centred as one.
pub(crate) fn card(o: &Opening, told: &str, bg: Bg, look: &Look, fonts: &Fonts, s: Scale) -> Vec<u8> {
    let dims = s.dims();
    let (w, h) = dims;
    let mut px = look.ground.repeat(w * h);
    let text =
        |t: &str, size: f32, fg: Rgb, bold: bool| video::text_at(t, size, fg, look.ground, bold, fonts, s);
    let name = text(&o.name, NAME_PX, look.ink, true);
    let line = text(&o.line, video::LINE_PX, look.ink, false);
    let code = (!o.code.is_empty()).then(|| {
        let lines: Vec<_> = o.code.iter().map(|l| spans(l, look.ink, bg)).collect();
        rows(&lines, CODE_PX, look.ground, fonts, s)
    });
    let told = text(told, TOLD_PX, colours(bg).faint, false);
    let (over_code, over_told) = (s.px(OVER_CODE), s.px(OVER_TOLD));
    let code_h = code.as_ref().map_or(0, |p| p.1 + over_code);
    let tall = name.1 + line.1 + code_h + over_told + told.1;
    let mut y = s.centre(h, tall);
    let mut centred = |p: &Picture, y: &mut usize| {
        video::blit(&mut px, dims, p, s.centre(w, p.0), *y, 255);
        *y += p.1;
    };
    centred(&name, &mut y);
    centred(&line, &mut y);
    if let Some(p) = &code {
        y += over_code;
        centred(p, &mut y);
    }
    y += over_told;
    centred(&told, &mut y);
    px
}
