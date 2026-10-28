//! The reader's property, for its loop in `tests.rs` and `fuzz/parsers`' `web` target: any bytes read — never a
//! panic, the limits held, every link's place a link in the text, every anchor on a line there. The page's own
//! server's (`facts.rs`), for the same loop and the `facts` target: on any page read, every request at every byte.
//! And the inputs tests build that tarae's tests build too (a gzip body, pictures): the crate's `check` feature, which
//! tarae's dev-dependency and `fuzz/parsers` turn on — never in tarae's own build.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::facts::{Facts, Known, PLACES_MAX, SYMBOLS_MAX, answer};
use super::{Prefer, Read, read};

/// libFuzzer's input: its first byte says how it's read — which part (`& 3`), and the content type (`>> 2 & 3`:
/// none — sniffed —, HTML, HTML in EUC-KR, plain text); the rest is the page.
pub fn page_input(b: &[u8]) {
    let Some((&how, bytes)) = b.split_first() else { return };
    let prefer = [Prefer::Guess, Prefer::Main, Prefer::Article, Prefer::Page][usize::from(how & 3)];
    let content_type = [None, Some("text/html"), Some("text/html; charset=euc-kr"), Some("text/plain")]
        [usize::from(how >> 2 & 3)];
    page(bytes, content_type, prefer);
}

/// Where the fuzz target stops asking at every byte of a page's text: one in a hundred past it. The property is
/// quadratic by design (each position × every request, each answer a walk of the page's facts): every byte of a long
/// input took the target to 26 runs a second, a tenth of the reader's.
const FUZZ_EVERY: usize = 4096;

/// libFuzzer's input for `facts`: as `page_input`'s — the page then asked of by its server.
pub fn facts_input(b: &[u8], now: Instant) {
    let Some((&how, bytes)) = b.split_first() else { return };
    let prefer = [Prefer::Guess, Prefer::Main, Prefer::Article, Prefer::Page][usize::from(how & 3)];
    let content_type = [None, Some("text/html"), Some("text/html; charset=euc-kr"), Some("text/plain")]
        [usize::from(how >> 2 & 3)];
    facts(bytes, content_type, prefer, FUZZ_EVERY, now);
}

/// The page's own server on `bytes` read as `content_type` says: `gd`, `space k` and `gr` at every byte of its text
/// (past `every`, one byte in a hundred), past its end and at numbers no text has, `space s` once, an unknown method,
/// requests with what isn't a position — never a panic; every position it answers in the text, on a character
/// boundary, as a rope of that text counts lines (the buffer's: never the server's own count), and every heading on a
/// line of the rope that is one; a hover's words a string; the lists within `SYMBOLS_MAX` and `PLACES_MAX`.
/// (`now`: the caller's clock — a test's, the fuzz target's; ages are only words here)
pub fn facts(bytes: &[u8], content_type: Option<&str>, prefer: Prefer, every: usize, now: Instant) {
    let url = "https://example.com/a/b.html";
    let p = read(bytes, url, content_type, prefer);
    let rope = ropey::Rope::from_str(&p.text);
    let f = Arc::new(Facts::of(&p, &rope));
    let mut k = Known { keep: Duration::from_secs(900), ..Known::default() };
    k.pages.insert(url.into(), (Arc::downgrade(&f), Some(now)));
    // (a hover's other branches: a page open, a copy kept)
    let other = Arc::new(Facts::default());
    k.pages.insert("https://example.com/a/c.html".into(), (Arc::downgrade(&other), None));
    k.kept.insert("https://example.com/".into(), now);
    fn ago(_: Duration) -> String {
        "a while ago".into()
    }
    let text = p.text.as_str();
    let held = |method: &str, v: &Option<Value>| {
        let Some(v) = v else { return };
        let mut at = Vec::new();
        positions(v, &mut at);
        for (line, character) in at {
            assert!(line < rope.len_lines(), "{method}: line {line} of {}", rope.len_lines());
            let b = rope.line_to_byte(line) + character;
            assert!(b <= text.len() && text.is_char_boundary(b), "{method}: {line}:{character} → byte {b}");
        }
        match method {
            "textDocument/hover" => assert!(v["contents"]["value"].is_string(), "{v}"),
            "textDocument/references" => assert!(v.as_array().is_some_and(|a| a.len() <= PLACES_MAX)),
            _ => {}
        }
    };
    let ask = |method: &str, position: Value| {
        let params = json!({ "textDocument": { "uri": url }, "position": position });
        let v = answer(&k, method, &params, now, ago);
        held(method, &v);
    };
    let methods = ["textDocument/definition", "textDocument/hover", "textDocument/references", "no/such"];
    let len = text.len();
    for b in (0..=len + 1).filter(|&b| b < every || b % 100 == 0) {
        let (line, character) = f.position(b);
        if b <= len {
            let l = rope.byte_to_line(b);
            assert_eq!((line, character), (l, b - rope.line_to_byte(l)), "byte {b}'s position");
        }
        for m in methods {
            ask(m, json!({ "line": line, "character": character }));
        }
    }
    for h in &f.headings {
        let l = rope.get_line(h.line).map(String::from).unwrap_or_default();
        assert!(l.trim_start().starts_with('#'), "heading {:?} on line {}: {l:?}", h.text, h.line);
    }
    let lines = rope.len_lines();
    for position in [
        json!({ "line": 0, "character": len + 1 }),
        json!({ "line": lines, "character": 0 }),
        json!({ "line": u64::MAX, "character": u64::MAX }),
        json!({ "line": 0, "character": u64::MAX }),
        json!({ "line": -1, "character": 0.5 }),
        json!("here"),
        Value::Null,
    ] {
        for m in methods {
            ask(m, position.clone());
        }
    }
    let symbols =
        answer(&k, "textDocument/documentSymbol", &json!({ "textDocument": { "uri": url } }), now, ago);
    held("textDocument/documentSymbol", &symbols);
    assert!(count(symbols.as_ref().unwrap_or(&Value::Null)) <= SYMBOLS_MAX);
    for params in
        [json!({}), json!({ "textDocument": { "uri": 7 } }), json!({ "textDocument": { "uri": "x" } })]
    {
        assert_eq!(answer(&k, "textDocument/definition", &params, now, ago), None);
    }
    assert!(answer(&k, "initialize", &Value::Null, now, ago).is_some());
}

/// Every `{line, character}` in an answer.
fn positions(v: &Value, out: &mut Vec<(usize, usize)>) {
    match v {
        Value::Object(o) => {
            let n = |k: &str| o.get(k).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
            if let (Some(l), Some(c)) = (n("line"), n("character")) {
                out.push((l, c));
            }
            o.values().for_each(|v| positions(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| positions(v, out)),
        _ => {}
    }
}

/// The symbols in a `documentSymbol` answer, nested ones too.
fn count(v: &Value) -> usize {
    v.as_array().map_or(0, |a| a.iter().map(|s| 1 + count(&s["children"])).sum())
}

/// libFuzzer's input for `gzip`: its first byte says how much it may make (64 × it; 0: one byte), the rest the body.
pub fn gzip_input(b: &[u8]) -> &'static str {
    let Some((&m, rest)) = b.split_first() else { return "refused" };
    gzip(rest, (64 * usize::from(m)).max(1))
}

/// Whatever a gzip body holds: never a panic, never more than `max` bytes and one; one unpacked whole ends with its
/// CRC of what it made, one cut is `max` and one.
pub fn gzip(b: &[u8], max: usize) -> &'static str {
    match super::gzip::gunzip(b, max) {
        Ok(super::gzip::Unpacked::Whole(out)) => {
            assert!(out.len() <= max, "{} past {max}", out.len());
            assert_eq!(b[b.len() - 8..b.len() - 4], crc32fast::hash(&out).to_le_bytes(), "its check");
            "whole"
        }
        Ok(super::gzip::Unpacked::Cut(out)) => {
            assert_eq!(out.len(), max + 1);
            "cut"
        }
        Err(_) => "refused",
    }
}

/// `data` as gzip: a header with `flags`' fields, deflate, the trailer.
pub fn gzipped(data: &[u8], flags: u8) -> Vec<u8> {
    use super::gzip::{FCOMMENT, FEXTRA, FHCRC, FNAME};
    let mut b = vec![0x1f, 0x8b, 8, flags, 0, 0, 0, 0, 0, 3];
    if flags & FEXTRA != 0 {
        b.extend_from_slice(&[3, 0, b'a', b'b', b'c']);
    }
    if flags & FNAME != 0 {
        b.extend_from_slice(b"page.html\0");
    }
    if flags & FCOMMENT != 0 {
        b.extend_from_slice(b"a comment\0");
    }
    if flags & FHCRC != 0 {
        b.extend_from_slice(&[0, 0]);
    }
    b.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
    b.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b
}

/// `img` as a PNG, written by `image`'s own encoder.
fn png_of(img: image::RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A white PNG `w`×`h`.
pub fn png(w: u32, h: u32) -> Vec<u8> {
    png_of(image::RgbImage::from_pixel(w, h, image::Rgb([255, 255, 255])))
}

/// A PNG `w`×`h` of noise — one that compresses badly (xorshift, the same each time).
pub fn noise(w: u32, h: u32) -> Vec<u8> {
    let mut x: u32 = 0x9E37_79B9;
    png_of(image::RgbImage::from_fn(w, h, |_, _| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let [a, b, c, _] = x.to_le_bytes();
        image::Rgb([a, b, c])
    }))
}

/// A decompression bomb's stand-in: a PNG whose header says `w`×`h` (8-bit RGB) over a few bytes of data —
/// written by hand, as no encoder makes one.
pub fn bomb(w: u32, h: u32) -> Vec<u8> {
    fn crc(b: &[u8]) -> u32 {
        let mut c = !0u32;
        for &x in b {
            c ^= u32::from(x);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let body = [kind, data].concat();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    chunk(b"IHDR", &[&w.to_be_bytes()[..], &h.to_be_bytes(), &[8, 2, 0, 0, 0]].concat());
    chunk(b"IDAT", &[0x78, 0x9C, 0x63, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01]);
    chunk(b"IEND", &[]);
    out
}

/// `bytes` read as `content_type` says: what's read holds.
pub fn page(bytes: &[u8], content_type: Option<&str>, prefer: Prefer) {
    let p = read(bytes, "https://example.com/a/b.html?q#f", content_type, prefer);
    assert!(p.text.len() <= super::reader::TEXT_MAX, "{} bytes of text", p.text.len());
    if p.read == Read::None {
        assert!(p.text.is_empty() && p.said.is_some());
        return;
    }
    for l in &p.links {
        let md = p.text.get(l.from..l.to).unwrap_or_else(|| panic!("a link at {}..{}", l.from, l.to));
        assert!(md.starts_with('[') && md.ends_with(')'), "{md:?}");
    }
    let lines = p.text.lines().count();
    assert!(p.anchors.iter().all(|a| a.line <= lines), "an anchor past the text's {lines} lines");
    assert!(!p.title.contains('\n'), "{:?}", p.title);
}
