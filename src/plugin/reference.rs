//! The plugin API's reference, generated from `plugin-sdk/wit/tarae.wit` (PD1's H2): `docs/plugins-reference.md` here,
//! and `REFERENCE.md` in each crate `tarae plugin new` starts — there it's inside the user's project, where Claude
//! reads it. A hand parser of the subset the WIT uses — one interface of records, enums and variants, one world of
//! exports, a construct a line — that fails on anything else, so a new kind of type can't be left out of it unseen.

use std::collections::BTreeSet;

use super::examples::EXAMPLES;

/// What the WIT declares, in its order.
#[derive(Debug, Default)]
struct Api {
    package: String,
    /// The file's first comment (its `//` lines before `package`), but the licence's line.
    intro: Vec<String>,
    types: Vec<Type>,
    exports: Vec<Item>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Record,
    Enum,
    Variant,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Record => "record",
            Kind::Enum => "enum",
            Kind::Variant => "variant",
        }
    }
}

#[derive(Debug)]
struct Type {
    kind: Kind,
    name: String,
    docs: Vec<String>,
    items: Vec<Item>,
}

/// A field (its type), a case (what it carries, if anything) or an export (its signature).
#[derive(Debug)]
struct Item {
    name: String,
    ty: Option<String>,
    docs: Vec<String>,
}

/// The WIT's own types, which name nothing of the API's.
const BUILTIN: &[&str] = &[
    "bool", "char", "string", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "f32", "f64", "list",
    "option", "tuple", "result", "func",
];

/// A name as WIT writes it (`%from` is `from`, escaped): lowercase words joined by `-`.
fn ident(s: &str) -> Option<String> {
    let s = s.strip_prefix('%').unwrap_or(s);
    let ok = s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.ends_with('-');
    ok.then(|| s.to_string())
}

/// `<kind> <name> {`, for the kinds known.
fn opens(line: &str, kinds: &[&str]) -> Option<(String, String)> {
    let rest = line.strip_suffix('{')?.trim_end();
    let (kind, name) = rest.split_once(' ')?;
    (kinds.contains(&kind)).then_some(())?;
    Some((kind.to_string(), ident(name.trim())?))
}

fn parse(wit: &str) -> Result<Api, String> {
    enum At {
        Top,
        Interface,
        Type,
        World,
    }
    let mut api = Api::default();
    let (mut at, mut docs, mut interfaces) = (At::Top, Vec::new(), 0);
    let mut used = Vec::new();
    for (i, raw) in wit.lines().enumerate() {
        let line = raw.trim();
        let unknown = || format!("tarae.wit:{}: not a construct the reference knows — `{line}`", i + 1);
        if let Some(doc) = line.strip_prefix("///") {
            docs.push(doc.strip_prefix(' ').unwrap_or(doc).to_string());
            continue;
        }
        if let Some(c) = line.strip_prefix("//") {
            if api.package.is_empty() && !c.contains("SPDX-License-Identifier") {
                api.intro.push(c.trim().to_string());
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        // (a doc comment documents what follows it: a line it can't document has none)
        let mut take = || std::mem::take(&mut docs);
        match at {
            At::Top => {
                if let Some(p) = line.strip_prefix("package ").and_then(|p| p.strip_suffix(';')) {
                    api.package = p.trim().to_string();
                } else if let Some((_, _)) = opens(line, &["interface"]) {
                    interfaces += 1;
                    if interfaces > 1 {
                        return Err(format!("{} (a second interface)", unknown()));
                    }
                    at = At::Interface;
                } else if opens(line, &["world"]).is_some() {
                    at = At::World;
                } else {
                    return Err(unknown());
                }
                if !take().is_empty() {
                    return Err(format!("tarae.wit:{}: a doc comment on `{line}`, which has none", i + 1));
                }
            }
            At::Interface => match (line, opens(line, &["record", "enum", "variant"])) {
                ("}", _) => at = At::Top,
                (_, Some((kind, name))) => {
                    let kind = match kind.as_str() {
                        "record" => Kind::Record,
                        "enum" => Kind::Enum,
                        _ => Kind::Variant,
                    };
                    api.types.push(Type { kind, name, docs: take(), items: Vec::new() });
                    at = At::Type;
                }
                _ => return Err(unknown()),
            },
            At::Type => {
                let Some(t) = api.types.last_mut() else { return Err(unknown()) };
                if line == "}" {
                    at = At::Interface;
                    continue;
                }
                let item = line.strip_suffix(',').ok_or_else(unknown)?;
                let (name, ty) = match t.kind {
                    Kind::Record => {
                        let (n, ty) = item.split_once(':').ok_or_else(unknown)?;
                        (n.trim(), Some(ty.trim()))
                    }
                    Kind::Enum => (item, None),
                    Kind::Variant => match item.split_once('(') {
                        Some((n, rest)) => (n, Some(rest.strip_suffix(')').ok_or_else(unknown)?)),
                        None => (item, None),
                    },
                };
                let name = ident(name).ok_or_else(unknown)?;
                t.items.push(Item { name, ty: ty.map(str::to_string), docs: take() });
            }
            At::World => {
                if line == "}" {
                    at = At::Top;
                } else if let Some(names) = line.strip_prefix("use ").and_then(|u| u.strip_suffix("};")) {
                    let (_, names) = names.split_once(".{").ok_or_else(unknown)?;
                    used.extend(names.split(',').map(|n| (i + 1, n.trim().to_string())));
                } else if let Some(e) = line.strip_prefix("export ").and_then(|e| e.strip_suffix(';')) {
                    let (name, sig) = e.split_once(':').ok_or_else(unknown)?;
                    let sig = sig.trim();
                    if !sig.starts_with("func(") {
                        return Err(unknown());
                    }
                    let name = ident(name.trim()).ok_or_else(unknown)?;
                    api.exports.push(Item { name, ty: Some(sig.to_string()), docs: take() });
                } else {
                    return Err(unknown());
                }
            }
        }
    }
    if !docs.is_empty() || !matches!(at, At::Top) {
        return Err("tarae.wit ends inside a block, or with a doc comment on nothing".into());
    }
    if api.package.is_empty() || api.exports.is_empty() {
        return Err("tarae.wit has no package, or its world exports nothing".into());
    }
    // Every name a type, a `use` or a signature names is one of the types declared
    let names: BTreeSet<&str> = api.types.iter().map(|t| t.name.as_str()).collect();
    for (line, n) in &used {
        if !names.contains(n.as_str()) {
            return Err(format!("tarae.wit:{line}: `use` names `{n}`, which isn't declared"));
        }
    }
    let typed = api.types.iter().flat_map(|t| &t.items).chain(&api.exports);
    for ty in typed.filter_map(|item| item.ty.as_deref()) {
        for (word, param) in words(ty) {
            if !param && !names.contains(word) && !BUILTIN.contains(&word) {
                return Err(format!("tarae.wit: `{ty}` names `{word}`, which isn't declared"));
            }
        }
    }
    Ok(api)
}

/// The names in a type or a signature, each with whether it's a parameter's (followed by `:`).
fn words(ty: &str) -> Vec<(&str, bool)> {
    let is = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '%';
    // (a name starts with a letter: `->` is none)
    let starts = |c: char| c.is_ascii_alphabetic() || c == '%';
    let mut out = Vec::new();
    let mut rest = ty;
    while let Some(start) = rest.find(starts) {
        let tail = &rest[start..];
        let end = tail.find(|c| !is(c)).unwrap_or(tail.len());
        let param = tail[end..].trim_start().starts_with(':');
        out.push((tail[..end].trim_start_matches('%'), param));
        rest = &tail[end..];
    }
    out
}

/// A type or a signature in Markdown: code, the API's own types linked to their sections.
fn linked(ty: &str, names: &BTreeSet<&str>) -> String {
    let is = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '%';
    let starts = |c: char| c.is_ascii_alphabetic() || c == '%';
    let (mut out, mut code) = (String::new(), String::new());
    let flush = |out: &mut String, code: &mut String| {
        if !code.is_empty() {
            out.push_str(&format!("`{}`", std::mem::take(code)));
        }
    };
    let mut rest = ty;
    while !rest.is_empty() {
        let end = match rest.starts_with(starts) {
            true => rest.find(|c| !is(c)).unwrap_or(rest.len()),
            false => rest.find(starts).unwrap_or(rest.len()),
        };
        let (tok, tail) = rest.split_at(end);
        let word = tok.trim_start_matches('%');
        if names.contains(word) && !tail.trim_start().starts_with(':') {
            flush(&mut out, &mut code);
            out.push_str(&format!("[`{word}`](#{word})"));
        } else {
            code.push_str(word);
        }
        rest = tail;
    }
    flush(&mut out, &mut code);
    out
}

/// Doc lines as one table cell (a `|` would end it).
fn cell(docs: &[String]) -> String {
    docs.join(" ").replace('|', "\\|")
}

/// The reference's text, from `wit` — Err: a construct it doesn't know, or a name not declared.
pub fn reference(wit: &str) -> Result<String, String> {
    let api = parse(wit)?;
    let names: BTreeSet<&str> = api.types.iter().map(|t| t.name.as_str()).collect();
    let mut anchors = BTreeSet::new();
    let mut anchor = |a: String| match anchors.insert(a.clone()) {
        true => Ok(a),
        false => Err(format!("tarae.wit: two names make the anchor #{a}")),
    };
    let mut out = format!(
        "# tarae's plugin API — reference\n\n{}\n\nThe package is `{}`. This is generated from `wit/tarae.wit`, with \
         its doc comments; a plugin's commands, events, permissions and answers are all here.\n",
        api.intro.join(" "),
        api.package
    );
    out.push_str("\n## What a plugin exports\n\ntarae calls these; a plugin answers with actions.\n\n");
    out.push_str("| export | signature | |\n|---|---|---|\n");
    for e in &api.exports {
        let id = anchor(format!("export-{}", e.name))?;
        let sig = linked(e.ty.as_deref().unwrap_or_default(), &names);
        out.push_str(&format!("| <a id=\"{id}\"></a>`{}` | {sig} | {} |\n", e.name, cell(&e.docs)));
    }
    out.push_str("\n## Types\n");
    for t in &api.types {
        anchor(t.name.clone())?;
        out.push_str(&format!("\n### {}\n\n*{}*", t.name, t.kind.word()));
        if !t.docs.is_empty() {
            out.push_str(&format!(" — {}", t.docs.join(" ")));
        }
        out.push_str("\n\n");
        out.push_str(match t.kind {
            Kind::Record => "| field | type | |\n|---|---|---|\n",
            Kind::Enum => "| case | |\n|---|---|\n",
            Kind::Variant => "| case | carries | |\n|---|---|---|\n",
        });
        for item in &t.items {
            let id = anchor(format!("{}-{}", t.name, item.name))?;
            let ty = item.ty.as_deref().map(|ty| linked(ty, &names)).unwrap_or_default();
            let first = format!("<a id=\"{id}\"></a>`{}`", item.name);
            out.push_str(&match t.kind {
                Kind::Enum => format!("| {first} | {} |\n", cell(&item.docs)),
                _ => format!("| {first} | {ty} | {} |\n", cell(&item.docs)),
            });
        }
    }
    Ok(out)
}

/// `docs/plugins-reference.md`: the reference, the examples linked where they are in this repository.
#[cfg(test)]
fn docs(wit: &str) -> Result<String, String> {
    let mut out = reference(wit)?;
    out.push_str(
        "\n## Examples\n\nWhole plugins, one idea each, with their tests ([`plugin-sdk/examples`](../plugin-sdk/examples)); \
         `tarae plugin new <dir> --from <example>` starts a crate from one. The guide is [plugins.md](plugins.md).\n\n",
    );
    for e in EXAMPLES {
        out.push_str(&format!("- [`{0}`](../plugin-sdk/examples/{0}/src/lib.rs) — {1}\n", e.name, e.idea));
    }
    out.push_str(&generated("docs/plugins-reference.md"));
    Ok(out)
}

/// `REFERENCE.md` in a crate `tarae plugin new` starts: the reference, then the examples' index on one line — each
/// printed by `tarae plugin example <name>`.
pub fn for_crate(wit: &str) -> Result<String, String> {
    let mut out = reference(wit)?;
    let names: Vec<String> = EXAMPLES.iter().map(|e| format!("`{}` ({})", e.name, e.idea)).collect();
    out.push_str(&format!(
        "\n## Examples\n\n`tarae plugin example <name>` prints one: {}.\n",
        names.join(" · ")
    ));
    out.push_str(&generated("REFERENCE.md"));
    Ok(out)
}

fn generated(what: &str) -> String {
    format!(
        "\n<!-- {what}: generated by tarae (src/plugin/reference.rs) from wit/tarae.wit — don't edit by hand -->\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIT: &str = include_str!("../../plugin-sdk/wit/tarae.wit");

    /// `docs/plugins-reference.md` is the WIT's reference as this tarae generates it — `TARAE_BLESS=1` rewrites it.
    #[test]
    fn plugins_reference_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/plugins-reference.md");
        crate::settings::check_generated(path, &docs(WIT).unwrap(), "plugins_reference");
    }

    /// Everything the WIT declares is in it: each type, each field and case, each export, with its doc comment.
    #[test]
    fn the_reference_has_every_name_and_its_docs() {
        let api = parse(WIT).unwrap();
        let text = reference(WIT).unwrap();
        assert_eq!(api.exports.len(), 6, "the world's exports");
        for t in &api.types {
            assert!(text.contains(&format!("\n### {}\n", t.name)), "{}", t.name);
            for item in &t.items {
                assert!(
                    text.contains(&format!("<a id=\"{}-{}\"></a>", t.name, item.name)),
                    "{}.{}",
                    t.name,
                    item.name
                );
            }
        }
        // (a doc comment of a case, one of a field, one of a type)
        assert!(text.contains("Hear the keys you press as commands"));
        assert!(text.contains("Milliseconds since 1970-01-01 UTC."));
        assert!(text.contains("*record* — The buffer a call is about."));
        // (`%from` is `from`; a named type is linked, the rest is code)
        assert!(text.contains("<a id=\"edit-from\"></a>`from` | `u64`"));
        assert!(text.contains("| `func() -> list<`[`command`](#command)`>` |"), "{text}");
        assert!(
            text.contains("`func(name: string, cx: `[`context`](#context)`) -> list<`[`action`](#action)`>`")
        );
    }

    /// What the parser doesn't know fails it, named with its line — never left out of the reference unseen.
    #[test]
    fn the_parser_refuses_what_it_doesnt_know() {
        let with =
            |extra: &str| WIT.replacen("interface types {", &format!("interface types {{\n{extra}"), 1);
        for (extra, says) in [
            ("    flags f {\n        a,\n    }", "`flags f {`"),
            ("    resource r {\n    }", "`resource r {`"),
            ("    type t = u64;", "`type t = u64;`"),
            ("    record r {\n        a: missing,\n    }", "names `missing`"),
            ("    record r {\n        a: u64\n    }", "`a: u64`"),
        ] {
            let e = reference(&with(extra)).unwrap_err();
            assert!(e.contains(says), "{extra}: {e}");
        }
        let e = reference(&format!("{WIT}\ninterface more {{\n}}\n")).unwrap_err();
        assert!(e.contains("a second interface"), "{e}");
        let e = reference(&WIT.replacen("    export run:", "    import run:", 1)).unwrap_err();
        assert!(e.contains("`import run:"), "{e}");
        let e = reference(&WIT.replacen("use types.{command,", "use types.{commands,", 1)).unwrap_err();
        assert!(e.contains("`commands`"), "{e}");
    }

    /// What a new crate gets: the reference, and the examples' index on one line, each by the command that prints it.
    #[test]
    fn a_crates_reference_ends_with_the_examples() {
        let text = for_crate(WIT).unwrap();
        let last = text.lines().rev().find(|l| l.starts_with("`tarae plugin example")).unwrap();
        for e in EXAMPLES {
            assert!(last.contains(&format!("`{}` ({})", e.name, e.idea)), "{last}");
        }
        assert!(!text.contains("](../"), "no link that leads out of the crate");
    }
}
