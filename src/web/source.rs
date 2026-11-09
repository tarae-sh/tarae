//! A wiki's pages through its REST API (the work-wiki plan W2): `runtime/web/sources.toml`'s rows, data, applied only
//! to a wiki `web.signed-in` names with that `kind` (`signed.rs`). A page's address below the base maps to the API's
//! request; its JSON answer gives the page's HTML and title (pointers), made a page the reader reads as any — at the
//! page's own address, so its links resolve as the wiki's and follow, replay and `gd` key on it. A search's address
//! reads as a list of pages at the wiki. Pure: the fetch asks (`fetch::page`).

use std::sync::OnceLock;

use serde_json::Value;

use super::signed::Wiki;

/// One row of the table.
#[derive(Debug)]
pub struct Row {
    page: String,
    api: String,
    html: Option<String>,
    title: String,
    list: Option<String>,
    space: Option<String>,
    link: Option<String>,
    short: Option<String>,
    check: Option<String>,
}

const TABLE: &str = include_str!("../../runtime/web/sources.toml");

/// The table: each kind's rows, in order.
fn table() -> &'static [(String, Vec<Row>)] {
    static T: OnceLock<Vec<(String, Vec<Row>)>> = OnceLock::new();
    T.get_or_init(|| parse(TABLE).unwrap_or_else(|e| panic!("runtime/web/sources.toml: {e}")))
}

fn parse(text: &str) -> Result<Vec<(String, Vec<Row>)>, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (kind, rows) in t {
        let rows = rows.as_array().ok_or_else(|| format!("{kind}: a list of rows"))?;
        let mut of = Vec::new();
        for r in rows {
            let r = r.as_table().ok_or_else(|| format!("{kind}: a row is a table"))?;
            let s = |k: &str| r.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let need = |k: &str| s(k).ok_or_else(|| format!("{kind}: a row without its {k}"));
            let row = Row {
                page: need("page")?,
                api: need("api")?,
                html: s("html"),
                title: need("title")?,
                list: s("list"),
                space: s("space"),
                link: s("link"),
                short: s("short"),
                check: s("check"),
            };
            if row.html.is_none() == row.list.is_none() {
                return Err(format!("{kind} {}: one of html and list", row.page));
            }
            if row.short.is_some() != row.check.is_some() {
                return Err(format!("{kind} {}: short and check together", row.page));
            }
            of.push(row);
        }
        out.push((kind, of));
    }
    Ok(out)
}

/// The kinds the table knows (`web.signed-in`'s `kind`).
pub fn kinds() -> impl Iterator<Item = &'static str> {
    table().iter().map(|(k, _)| k.as_str())
}

fn rows(kind: &str) -> &'static [Row] {
    table().iter().find(|(k, _)| k == kind).map_or(&[], |(_, r)| r.as_slice())
}

/// A kind's search, as an address below the base with `<words>` for the words (the chat's line, W4).
pub fn search_form(kind: &str) -> Option<String> {
    let row = rows(kind).iter().find(|r| r.list.is_some())?;
    let (path, query) = row.page.split_once('?').unwrap_or((&row.page, ""));
    let query = query.split('&').map(|kv| match kv.split_once('=') {
        Some((k, v)) if v.starts_with('{') => format!("{k}=<words>"),
        _ => kv.to_string(),
    });
    Some(format!("{path}?{}", query.collect::<Vec<_>>().join("&")))
}

/// A page's address mapped: the API's address, and what reads its answer.
#[derive(Debug)]
pub struct Asked {
    pub api: String,
    pub wiki: Wiki,
    row: &'static Row,
    /// The page's path below the base (a short link's check).
    below: String,
    /// A search's words, for its list's title.
    words: Option<String>,
}

/// `url` as a source request — None: not under a named wiki's base, or no row of its kind takes it (fetched as a page);
/// Err: a row took it but its values can't be asked (a short link that isn't one).
pub fn asked(url: &str, wikis: &[&Wiki]) -> Option<Result<Asked, String>> {
    let t = super::http::target(url).ok()?;
    let wiki = wikis.iter().find(|w| w.under(&t))?;
    let below = t.path.strip_prefix(wiki.prefix.as_str())?;
    let below = if below.is_empty() { "/" } else { below };
    let (path, query) = below.split_once('?').unwrap_or((below, ""));
    for row in rows(&wiki.kind) {
        let Some(mut got) = matched(&row.page, path, query) else { continue };
        if let Some(name) = &row.short {
            let Some(id) = got.iter().find(|(k, _)| k == name).and_then(|(_, v)| tiny_id(v)) else {
                return Some(Err(unresolved(url)));
            };
            got.retain(|(k, _)| k != name);
            got.push((name.clone(), id.to_string()));
        }
        // (a value in the API's path, not its query, is one segment: never a slash or a dot segment)
        let path_part = row.api.split('?').next().unwrap_or_default();
        let strays = |(k, v): &(String, String)| {
            path_part.contains(&format!("{{{k}}}")) && (v.contains(['/', '\\']) || v == "." || v == "..")
        };
        if got.iter().any(strays) {
            return Some(Err(format!("{url}: not a page address tarae reads through the wiki's API")));
        }
        let words = row.list.as_ref().and_then(|_| got.first().map(|(_, v)| v.clone()));
        let api = format!("{}{}", wiki.base, filled(&row.api, &got));
        return Some(Ok(Asked { api, wiki: (*wiki).clone(), row, below: path.to_string(), words }));
    }
    None
}

/// The words when a short link doesn't resolve to the page it names.
fn unresolved(url: &str) -> String {
    format!("{url}: its short link couldn't be resolved here — :web-open opens it in your browser")
}

/// The values `pattern` takes from a path and query below the base, or None where it doesn't match.
fn matched(pattern: &str, path: &str, query: &str) -> Option<Vec<(String, String)>> {
    let (ppath, pquery) = pattern.split_once('?').unwrap_or((pattern, ""));
    let mut got = Vec::new();
    let (want, have): (Vec<&str>, Vec<&str>) = (ppath.split('/').collect(), path.split('/').collect());
    if want.len() != have.len() {
        return None;
    }
    for (w, h) in want.iter().zip(&have) {
        match w.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            Some(name) if !h.is_empty() => got.push((name.to_string(), decoded(h))),
            Some(_) => return None,
            None if w == h => {}
            None => return None,
        }
    }
    let params: Vec<(&str, &str)> =
        query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
    for kv in pquery.split('&').filter(|p| !p.is_empty()) {
        let (k, w) = kv.split_once('=').unwrap_or((kv, ""));
        let v = params.iter().find(|(pk, _)| *pk == k).map(|(_, v)| *v)?;
        match w.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            Some(name) if !v.is_empty() => got.push((name.to_string(), decoded(v))),
            Some(_) => return None,
            None if w == v => {}
            None => return None,
        }
    }
    Some(got)
}

/// A value as a form writes it: `+` a space, `%xx` its byte — UTF-8, else replaced.
fn decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `s` percent-encoded as a query's value: only the unreserved characters as they are.
fn encoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(char::from(b)),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The API's template with the values in: `{name}` encoded, `{name:cql}` a CQL site search of its words.
fn filled(template: &str, got: &[(String, String)]) -> String {
    let mut out = template.to_string();
    for (k, v) in got {
        out = out.replace(&format!("{{{k}}}"), &encoded(v));
        let cql = format!("siteSearch ~ \"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
        out = out.replace(&format!("{{{k}:cql}}"), &encoded(&cql));
    }
    out
}

/// A Confluence short link's id (`/x/kIA8`) → the page id it encodes: base64 (`-` `_` or `+` `/`) of its
/// little-endian bytes, the trailing zeros dropped — observed on Atlassian's own examples, not documented; the answer's
/// `_links.tinyui` checks it (`check`).
fn tiny_id(s: &str) -> Option<u64> {
    let six = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' | b'+' => Some(62),
        b'_' | b'/' => Some(63),
        _ => None,
    };
    let s = s.trim_end_matches('=');
    if s.is_empty() || s.len() > 11 {
        return None;
    }
    let mut bits: u128 = 0;
    for (i, c) in s.bytes().enumerate() {
        bits |= u128::from(six(c)?) << (128 - 6 * (i + 1));
    }
    let bytes = bits.to_be_bytes();
    let mut id: u64 = 0;
    for (i, b) in bytes[..8].iter().enumerate() {
        id |= u64::from(*b) << (8 * i);
    }
    (id > 0 && bytes[8..].iter().all(|b| *b == 0)).then_some(id)
}

/// The page the API's answer gives, as HTML the reader takes (its title in `<title>` and a heading) — or why not, in
/// words: a 404 or no result is "no such page … or not one your account can see" (the reference's own 404).
pub fn page_of(a: &Asked, url: &str, status: u16, bytes: &[u8]) -> Result<String, String> {
    let host = &a.wiki.host;
    let none = || format!("no such page at {host}, or not one your account can see");
    match status {
        404 => return Err(none()),
        // (what a real Data Center answers isn't checked here yet: each surprise says itself)
        406 => return Err(format!("the wiki refused the JSON request (406) for {url}")),
        200..=299 => {}
        s => return Err(format!("{host}'s API answered {s} for {url}")),
    }
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|_| format!("{host}'s API answered what isn't JSON for {url} — a sign-in page?"))?;
    let text = |p: &str, v: &Value| v.pointer(p).and_then(Value::as_str).map(str::to_string);
    let esc =
        |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let r = a.row;
    if let Some(list) = &r.list {
        let words = a.words.clone().unwrap_or_default();
        let title = format!("{words} — {}'s search", a.wiki.shown());
        let items = v.pointer(list).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut body = String::new();
        for it in &items {
            let Some(name) = text(&r.title, it) else { continue };
            let link = r.link.as_ref().and_then(|p| text(p, it));
            let space = r.space.as_ref().and_then(|p| text(p, it));
            let name = match link {
                Some(l) if l.starts_with('/') => {
                    format!("<a href=\"{}{}\">{}</a>", a.wiki.base, esc(&l), esc(&name))
                }
                _ => esc(&name),
            };
            let space = space.map(|s| format!(" · {}", esc(&s))).unwrap_or_default();
            body.push_str(&format!("<li>{name}{space}</li>\n"));
        }
        let body = match body.is_empty() {
            true => "<p>Nothing found for those words.</p>".to_string(),
            false => format!("<ul>\n{body}</ul>"),
        };
        return Ok(html(&esc(&title), &body));
    }
    if let Some(check) = &r.check
        && text(check, &v).as_deref() != Some(a.below.as_str())
    {
        return Err(unresolved(url));
    }
    let body = r.html.as_ref().and_then(|p| text(p, &v)).ok_or_else(none)?;
    let body = match body.trim().is_empty() {
        true => format!("<p>(the wiki's API gave this page an empty {})</p>", body_name(r)),
        false => body,
    };
    let title = text(&r.title, &v).unwrap_or_default();
    Ok(html(&esc(&title), &body))
}

/// The body a row reads, as the API names it (`/body/view/value` → `body.view`).
fn body_name(r: &Row) -> String {
    let p = r.html.as_deref().unwrap_or_default();
    let named = |s: &&str| !s.is_empty() && !matches!(*s, "value" | "results") && s.parse::<usize>().is_err();
    p.split('/').filter(named).collect::<Vec<_>>().join(".")
}

fn html(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head><body><main>\
         <h1>{title}</h1>\n{body}\n</main></body></html>"
    )
}

#[cfg(test)]
mod tests;
