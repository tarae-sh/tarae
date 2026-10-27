//! A doc comment's text as Markdown, for `doccomment::chunks`: a block of HTML (Javadoc's — `DocComments.html`)
//! and the inline tags `{@code …}` and block tags `@param …` its pack lists, so one renderer draws every
//! language's docs and a `{@link Foo}` looks as rustdoc's ``[`Foo`]`` does. Pure, and anyone's text: never a
//! panic, every index on a character's boundary.
//!
//! What HTML means is the standard's, not a language's: `<p>`, `<br>`, a blank line and the block tags break a
//! paragraph; `<ul>`/`<ol>`/`<li>` are items, `<h1>`–`<h6>` headings, `<pre>` a code block with its lines (the
//! `<pre>{@code …}</pre>` idiom too); `<code>` and its kin code, `<b>` bold, `<i>`/`<em>` italic, `<a href>` link
//! text; entities decoded. A `<` that opens no known tag stays text (`@param <K>`, `List<String>` in prose).

use super::{DocComments, InlineKind, TagArg};

/// A block's bodies (HTML) → Markdown lines: its description, then a line per block tag (`@name …`).
pub fn markdown(bodies: &[String], syn: &DocComments) -> Vec<String> {
    let (desc, tags) = sections(bodies);
    let mut out = convert(&desc, syn);
    if !tags.is_empty() {
        out.push(String::new());
    }
    for (name, text) in tags {
        out.push(tag_line(&name, &text, syn, true));
    }
    out
}

/// One Markdown line of a block (not a code fence's): a block tag's argument told apart (`tag_line`), the inline
/// tags its pack lists rewritten.
pub fn markdown_line(s: &str, syn: &DocComments) -> String {
    let t = s.trim_start();
    if let Some((name, rest)) = tag_start(t) {
        return tag_line(name, rest, syn, false);
    }
    inline_tags(s, syn, false)
}

/// `@name rest` at a line's start: (name, rest).
fn tag_start(t: &str) -> Option<(&str, &str)> {
    let rest = t.strip_prefix('@')?;
    let n = rest.find(|c: char| !c.is_alphanumeric()).unwrap_or(rest.len());
    let after = &rest[n..];
    (n > 0
        && rest.starts_with(|c: char| c.is_alphabetic())
        && after.chars().next().is_none_or(char::is_whitespace))
    .then(|| (&rest[..n], after))
}

/// The description and the block tags `(name, text)`: a tag starts at a line beginning `@name` outside a `<pre>`
/// (where `@Override` is code) and runs to the next.
fn sections(bodies: &[String]) -> (String, Vec<(String, String)>) {
    let mut desc = String::new();
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut pre = 0usize;
    for b in bodies {
        let into = match tag_start(b.trim_start()).filter(|_| pre == 0) {
            Some((name, rest)) => {
                tags.push((name.to_string(), rest.trim_start().to_string()));
                None
            }
            None => Some(tags.last_mut().map_or(&mut desc, |t| &mut t.1)),
        };
        if let Some(into) = into {
            if !into.is_empty() {
                into.push('\n');
            }
            into.push_str(b);
        }
        let lower = b.to_ascii_lowercase();
        pre = (pre + lower.matches("<pre").count()).saturating_sub(lower.matches("</pre").count());
    }
    (desc, tags)
}

/// A block tag as one Markdown line: `@name`, its argument by its pack's `TagArg` (a name as code, a reference as
/// a link, a JSDoc `{Type}` as code), then its description — `html`: read as HTML, else as Markdown.
fn tag_line(name: &str, raw: &str, syn: &DocComments, html: bool) -> String {
    let arg = syn.tag(name).map_or(TagArg::None, |t| t.arg);
    let mut rest = raw.trim_start();
    let mut head = Vec::new();
    match arg {
        TagArg::Name => {
            if rest.starts_with('{')
                && !rest.starts_with("{@")
                && let Some(e) = closing_brace(rest)
            {
                head.push(code_span(rest[1..e].trim()));
                rest = rest[e + 1..].trim_start();
            }
            let n = rest.find(char::is_whitespace).unwrap_or(rest.len());
            if n > 0 {
                head.push(code_span(&rest[..n]));
                rest = &rest[n..];
            }
        }
        // (`@see "a title"`, `@see <a href=…>`, a Markdown link: no reference first)
        TagArg::Ref if !rest.starts_with(['<', '"', '[']) => {
            let (r, after) = split_ref(rest);
            if !r.is_empty() {
                head.push(link(&code_span(&short_ref(r))));
                rest = after;
            }
        }
        _ => {}
    }
    let desc = match html {
        true => {
            let lines = convert(rest, syn);
            let text = lines.iter().filter(|l| !l.starts_with("```")).map(|l| l.trim());
            text.filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
        }
        false => inline_tags(rest.trim(), syn, false),
    };
    head.push(desc);
    head.retain(|h| !h.is_empty());
    format!("@{name} {}", head.join(" ")).trim_end().to_string()
}

/// `{@name body}` rewritten where its pack lists `name` (`html`: the text around it escaped as HTML's text is).
fn inline_tags(s: &str, syn: &DocComments, html: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(at) = s[i..].find("{@").map(|a| i + a) {
        out.push_str(&s[i..at]);
        match inline_at(&s[at..], syn) {
            Some((len, kind, body)) => {
                out.push_str(&inline_md(kind, body, html));
                i = at + len;
            }
            None => {
                out.push_str("{@");
                i = at + 2;
            }
        }
    }
    out.push_str(&s[i..]);
    out
}

/// An inline tag at `s`'s start (`{@…}`, braces balanced) its pack lists: (its length, kind, body).
fn inline_at<'s>(s: &'s str, syn: &DocComments) -> Option<(usize, InlineKind, &'s str)> {
    let end = closing_brace(s)?;
    let inner = &s[2..end];
    let n = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let kind = syn.inline.iter().find(|(name, _)| *name == inner[..n]).map(|(_, k)| *k)?;
    // (one space or line break after the name is the separator; the rest is the body as written)
    let body = inner[n..].strip_prefix([' ', '\t', '\n']).unwrap_or(&inner[n..]);
    Some((end + 1, kind, body))
}

/// The byte index of the `}` closing the `{` at `s`'s start (nested pairs counted), if any.
fn closing_brace(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// An inline tag as Markdown — `html`: text escaped.
fn inline_md(kind: InlineKind, body: &str, html: bool) -> String {
    let text = |s: &str| if html { escape(&collapse(s)) } else { collapse(s) };
    match kind {
        InlineKind::Code => code_span(&collapse(body)),
        InlineKind::Link => link(&code_span(&collapse(&shown_ref(body)))),
        InlineKind::PlainLink => link(&text(&shown_ref(body))),
        InlineKind::Text => text(body),
    }
}

/// What an inline tag shows as plain text (inside `<pre>` or `<code>`).
fn inline_plain(kind: InlineKind, body: &str) -> String {
    match kind {
        InlineKind::Link | InlineKind::PlainLink => shown_ref(body),
        InlineKind::Code | InlineKind::Text => body.to_string(),
    }
}

/// A link's text: its label, else its reference shortened.
fn shown_ref(body: &str) -> String {
    let (r, label) = split_ref(body.trim());
    if label.trim().is_empty() { short_ref(r) } else { label.trim().to_string() }
}

/// A reference and what follows it: up to the first space outside parentheses (`#put(Object, Object) label`).
fn split_ref(s: &str) -> (&str, &str) {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth == 0 => return (&s[..i], s[i..].trim_start()),
            _ => {}
        }
    }
    (s, "")
}

/// A reference as javadoc shows it: a module and a package's names dropped (`java.base/java.util.Map.Entry` →
/// `Map.Entry`), a member after its class (`Cache#get(java.lang.Object)` → `Cache.get(Object)`, `#capacity()` →
/// `capacity()`).
fn short_ref(r: &str) -> String {
    let (class, member) = match r.split_once('#') {
        Some((c, m)) => (c, Some(m)),
        None => (r, None),
    };
    let class = short_type(class.rsplit_once('/').map_or(class, |(_, c)| c));
    let member = member.map(|m| match m.split_once('(') {
        Some((name, args)) => {
            let args = args.strip_suffix(')').unwrap_or(args);
            let args: Vec<String> = args.split(',').map(|a| short_type(a.trim())).collect();
            format!("{name}({})", args.join(", "))
        }
        None => m.to_string(),
    });
    match member {
        None => class,
        Some(m) if class.is_empty() => m,
        Some(m) => format!("{class}.{m}"),
    }
}

/// A type's name without its package (the names before the first that starts upper case — the last kept).
fn short_type(t: &str) -> String {
    let parts: Vec<&str> = t.split('.').collect();
    let first = parts.iter().position(|p| !p.starts_with(char::is_lowercase));
    parts[first.unwrap_or(parts.len() - 1)..].join(".")
}

/// `s` as a Markdown code span: as many backticks around it as it needs.
fn code_span(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut run = 0;
    let mut most = 0;
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        most = most.max(run);
    }
    let ticks = "`".repeat(most + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') { " " } else { "" };
    format!("{ticks}{pad}{s}{pad}{ticks}")
}

fn link(text: &str) -> String {
    if text.is_empty() { String::new() } else { format!("[{text}]()") }
}

/// Whitespace runs as one space.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text that means only itself in Markdown.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// HTML's tags tarae reads; another name after a `<` is text.
const TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "article",
    "aside",
    "b",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "main",
    "mark",
    "nav",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "section",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];

/// A tag at `s`'s start (`<name …>`, `</name>`) whose name is one of `TAGS`: (its length, closing, name, whether
/// it has an `href`).
fn tag_at(s: &str) -> Option<(usize, bool, String, bool)> {
    let rest = s.strip_prefix('<')?;
    let (closing, rest) = match rest.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let n = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    let name = rest[..n].to_ascii_lowercase();
    if !TAGS.contains(&name.as_str())
        || !rest[n..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/')
    {
        return None;
    }
    let end = rest[n..].find('>')? + n;
    let href = rest[n..end].to_ascii_lowercase().contains("href");
    Some((s.len() - rest.len() + end + 1, closing, name, href))
}

/// An entity at `s`'s start (`&lt;`, `&#123;`, `&#x7B;`): (its length, the character).
fn entity_at(s: &str) -> Option<(usize, char)> {
    let end = s.char_indices().take(12).find(|&(_, c)| c == ';')?.0;
    let name = &s[1..end];
    let c = match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "times" => '×',
        "bull" => '•',
        "rarr" => '→',
        "larr" => '←',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "sect" => '§',
        "para" => '¶',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        _ => {
            let n = name.strip_prefix('#')?;
            let v = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            // (a control character is never drawn from anyone's text)
            char::from_u32(v).filter(|c| !c.is_control())?
        }
    };
    Some((end + 1, c))
}

/// HTML → Markdown lines, as it's read.
struct Md {
    lines: Vec<String>,
    /// The paragraph being read, in Markdown, and what starts its line (`- ` an item, `### ` a heading).
    cur: String,
    lead: &'static str,
    /// A space is owed before the next text.
    space: bool,
    /// Inside `<code>`: its text as written.
    code: Option<String>,
    /// Inside `<pre>`: its text as written.
    pre: Option<String>,
    /// Inside `<a href>`.
    link: bool,
}

/// A description's HTML as Markdown lines (`Md`).
fn convert(text: &str, syn: &DocComments) -> Vec<String> {
    let mut md = Md {
        lines: Vec::new(),
        cur: String::new(),
        lead: "",
        space: false,
        code: None,
        pre: None,
        link: false,
    };
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let s = &text[i..];
        if c == '<' && s.starts_with("<!--") {
            i += s.find("-->").map_or(s.len(), |e| e + 3);
            continue;
        }
        if c == '<'
            && let Some((len, closing, name, href)) = tag_at(s)
        {
            md.tag(&name, closing, href);
            i += len;
            continue;
        }
        if c == '&'
            && let Some((len, ch)) = entity_at(s)
        {
            md.text(ch.encode_utf8(&mut [0; 4]));
            i += len;
            continue;
        }
        if s.starts_with("{@")
            && let Some((len, kind, body)) = inline_at(s, syn)
        {
            md.inline(kind, body);
            i += len;
            continue;
        }
        if c.is_whitespace() && md.pre.is_none() {
            let n = s.find(|c: char| !c.is_whitespace()).unwrap_or(s.len());
            md.blank(s[..n].matches('\n').count());
            i += n;
            continue;
        }
        md.text(c.encode_utf8(&mut [0; 4]));
        i += c.len_utf8();
    }
    md.end_pre();
    md.flush();
    while md.lines.last().is_some_and(|l| l.is_empty()) {
        md.lines.pop();
    }
    md.lines
}

impl Md {
    /// Text as it reads (one character or a decoded one).
    fn text(&mut self, s: &str) {
        if let Some(p) = &mut self.pre {
            p.push_str(s);
        } else if let Some(c) = &mut self.code {
            c.push_str(s);
        } else {
            self.owed();
            self.cur.push_str(&escape(s));
        }
    }

    /// Markdown as it is (a code span, a link).
    fn raw(&mut self, s: &str) {
        if !s.is_empty() {
            self.owed();
            self.cur.push_str(s);
        }
    }

    fn owed(&mut self) {
        if std::mem::take(&mut self.space) && !self.cur.is_empty() {
            self.cur.push(' ');
        }
    }

    /// A run of whitespace with `newlines` line breaks: a space, or between paragraphs (a blank line).
    fn blank(&mut self, newlines: usize) {
        match &mut self.code {
            Some(c) => c.push(' '),
            None if newlines >= 2 => self.gap(),
            None => self.space = true,
        }
    }

    /// Bold and italic marks: an opening one takes the space owed before it, a closing one leaves it after.
    fn mark(&mut self, m: &str, closing: bool) {
        if self.code.is_some() || self.pre.is_some() {
            return;
        }
        if !closing {
            self.owed();
        }
        self.cur.push_str(m);
    }

    fn inline(&mut self, kind: InlineKind, body: &str) {
        match (&mut self.pre, &mut self.code) {
            (Some(p), _) => p.push_str(&inline_plain(kind, body)),
            (None, Some(c)) => c.push_str(&inline_plain(kind, body)),
            (None, None) => self.raw(&inline_md(kind, body, true)),
        }
    }

    fn tag(&mut self, name: &str, closing: bool, href: bool) {
        if self.pre.is_some() {
            if name == "pre" && closing {
                self.end_pre();
                self.gap();
            }
            return;
        }
        match name {
            "pre" if !closing => {
                self.gap();
                self.pre = Some(String::new());
            }
            "code" | "tt" | "kbd" | "samp" | "var" => match (closing, self.code.take()) {
                (false, None) => self.code = Some(String::new()),
                (false, Some(c)) => self.code = Some(c),
                (true, Some(c)) => self.raw(&code_span(&collapse(&c))),
                (true, None) => {}
            },
            _ if self.code.is_some() => {}
            "b" | "strong" => self.mark("**", closing),
            "i" | "em" | "cite" | "dfn" => self.mark("*", closing),
            "a" if !closing && href && !self.link => {
                self.link = true;
                self.mark("[", false);
            }
            "a" if closing && self.link => {
                self.link = false;
                self.mark("]()", true);
            }
            "li" if !closing => {
                self.flush();
                self.lead = "- ";
            }
            "li" => self.flush(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !closing => {
                self.gap();
                self.lead = "### ";
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "br" | "ul" | "ol" | "div" | "blockquote"
            | "table" | "tr" | "dl" | "dt" | "dd" | "hr" | "center" | "section" | "article" | "header"
            | "footer" | "figure" | "details" | "address" | "nav" | "main" | "aside" => self.gap(),
            "td" | "th" => self.space = true,
            _ => {}
        }
    }

    /// The paragraph read so far as a line.
    fn flush(&mut self) {
        if let Some(c) = self.code.take() {
            self.raw(&code_span(&collapse(&c)));
        }
        if self.link {
            self.link = false;
            self.cur.push_str("]()");
        }
        let cur = std::mem::take(&mut self.cur);
        let lead = std::mem::take(&mut self.lead);
        self.space = false;
        if cur.trim().is_empty() {
            return;
        }
        // (a paragraph that starts as Markdown's blocks do means only its text)
        let bare = if cur.starts_with(['#', '-', '+', '@', '>']) { "\\" } else { "" };
        self.lines.push(format!("{lead}{bare}{}", cur.trim_end()));
    }

    /// A paragraph's end.
    fn gap(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.is_empty()) {
            self.lines.push(String::new());
        }
    }

    /// A `<pre>`'s text as a code block: its first and last blank lines left out, its common indentation too.
    fn end_pre(&mut self) {
        let Some(p) = self.pre.take() else { return };
        let mut lines: Vec<&str> = p.lines().map(str::trim_end).collect();
        while lines.first().is_some_and(|l| l.is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let lines: Vec<String> = lines.iter().map(|l| l.replace('\t', "    ")).collect();
        let lead = |l: &String| l.chars().take_while(|c| c.is_whitespace()).count();
        let indent = lines.iter().filter(|l| !l.is_empty()).map(lead).min().unwrap_or(0);
        self.lines.push("```".into());
        for l in &lines {
            self.lines.push(l.chars().skip(indent).collect());
        }
        self.lines.push("```".into());
    }
}

#[cfg(test)]
mod tests;
