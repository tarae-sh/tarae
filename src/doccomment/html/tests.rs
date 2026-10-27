use super::*;

/// Java's pack data, as tarae reads it.
fn java() -> &'static DocComments {
    crate::doccomment::syntax(Some("java")).expect("java's pack has doc comments")
}

fn bodies(text: &str) -> Vec<String> {
    text.lines().map(String::from).collect()
}

/// A class's Javadoc: bold and italic, paragraphs, a list, inline tags (a reference shortened), and its block
/// tags one line each after a blank line — a `<K>` that opens no HTML tag kept.
#[test]
fn a_javadoc_reads_as_markdown() {
    let doc = bodies(
        "A small <b>least-recently-used</b> cache.\n<p>\nEntries past {@link #capacity()} are evicted oldest first. Use\n\
         {@code Cache.of(16)} for a cache; see\n{@link java.util.LinkedHashMap} for the map it keeps.\n<ul>\n\
         \x20 <li>{@code get} moves an entry to the front</li>\n  <li>{@code put} may evict the <em>last</em> one</li>\n\
         </ul>\n\n@param <K> the type of keys\n@author Ada",
    );
    assert_eq!(
        markdown(&doc, java()),
        [
            "A small **least-recently-used** cache.",
            "",
            "Entries past [`capacity()`]() are evicted oldest first. Use `Cache.of(16)` for a cache; see \
             [`LinkedHashMap`]() for the map it keeps.",
            "",
            "- `get` moves an entry to the front",
            "- `put` may evict the *last* one",
            "",
            "@param `<K>` the type of keys",
            "@author Ada",
        ]
    );
}

/// `<pre>{@code …}</pre>`: a code block with its lines and their indentation, its braces balanced, an annotation
/// in it code and not a block tag; a tag's argument by its kind — a name as code, a reference (spaces inside its
/// parentheses) as a link — and a description over two lines joined.
#[test]
fn pre_blocks_keep_their_lines_and_tags_their_arguments() {
    let doc = bodies(
        "Returns the value for {@code key}.\n\n<pre>{@code\n@Override\nint f() {\n    return 1;\n}\n}</pre>\n\n\
         @param key  the key to look up,\n    never {@code null}\n@throws java.io.IOException if it fails\n\
         @see #put(java.lang.Object, Object) the other way",
    );
    assert_eq!(
        markdown(&doc, java()),
        [
            "Returns the value for `key`.",
            "",
            "```",
            "@Override",
            "int f() {",
            "    return 1;",
            "}",
            "```",
            "",
            "@param `key` the key to look up, never `null`",
            "@throws [`IOException`]() if it fails",
            "@see [`put(Object, Object)`]() the other way",
        ]
    );
}

/// Headings, entities, text that would mean something in Markdown kept as text, `<a href>` as a link, comments
/// dropped; a `{@link … label}` shows its label, `{@linkplain}` as plain link text, an unlisted tag as written.
#[test]
fn html_and_inline_tags_as_javadoc_shows_them() {
    let doc = bodies(
        "<h2>Use</h2>See <a href=\"x.html\">the guide</a> &mdash; a&lt;b &amp; snake_case *x*.<!-- not shown -->\n\
         {@link Cache#get(Object) the getter}, {@linkplain Map plain}, {@inheritDoc}, {@code a`b}",
    );
    assert_eq!(
        markdown(&doc, java()),
        [
            "### Use",
            "",
            "See [the guide]() — a<b & snake\\_case \\*x\\*. [`the getter`](), [plain](), {@inheritDoc}, ``a`b``",
        ]
    );
}

/// A Markdown doc (Java's `///`) keeps its Markdown; its inline tags and a block tag's argument are rewritten.
#[test]
fn a_markdown_line_takes_its_packs_tags() {
    assert_eq!(markdown_line("@param key the {@code k} *as is*", java()), "@param `key` the `k` *as is*");
    assert_eq!(markdown_line("Uses {@link java.util.List}.", java()), "Uses [`List`]().");
    assert_eq!(markdown_line("@custom stays", java()), "@custom stays");
    // (a `]` inside the link's code doesn't end the link)
    let link = markdown_line("{@link #m(int[])} after", java());
    let spans = crate::markdown::inline_spans(&link, &crate::theme::Theme::builtin());
    assert_eq!(spans.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["m(int[])", " after"], "{link}");
    // JSDoc's `{Type}` before the name
    let syn = DocComments {
        tags: vec![crate::doccomment::TagSpec {
            name: "param".into(),
            label: "Params".into(),
            arg: TagArg::Name,
        }],
        ..Default::default()
    };
    assert_eq!(markdown_line("@param {string} name who", &syn), "@param `string` `name` who");
}

#[test]
fn references_are_shortened_as_javadoc_does() {
    for (r, shown) in [
        ("#capacity()", "capacity()"),
        ("java.util.Map.Entry", "Map.Entry"),
        ("java.base/java.util.List", "List"),
        ("Cache#get(java.lang.Object, int[])", "Cache.get(Object, int[])"),
        ("#MAX", "MAX"),
        ("someFunction", "someFunction"),
        ("", ""),
    ] {
        assert_eq!(short_ref(r), shown, "{r}");
    }
    assert_eq!(split_ref("#put(Object, Object) label"), ("#put(Object, Object)", "label"));
    assert_eq!(split_ref("#m(a"), ("#m(a", ""));
}

/// Anyone's text: cut-short tags, entities and braces stay text, never a panic or a control character.
#[test]
fn hostile_text_stays_text() {
    for (text, shown) in [
        ("a < b && c", "a < b && c"),
        ("<b", "<b"),
        ("x &#0; &#xFFFFFFFF; &#x41; &", "x &#0; &#xFFFFFFFF; A &"),
        ("{@code {unbalanced", "{@code {unbalanced"),
        ("{@}", "{@}"),
        ("<p>éà <b>한</b>", "éà **한**"),
        ("- dash", "\\- dash"),
        ("<!-- open", ""),
    ] {
        let got = markdown(&bodies(text), java());
        assert_eq!(got.join("\n"), shown, "{text:?}");
    }
    // A `<pre>` never closed ends with the block
    assert_eq!(markdown(&bodies("<pre>\n  a\n    b"), java()), ["```", "a", "  b", "```"]);
    for text in ["<pre>　　x\n　y", "&", "{@link", "{@link }", "<a href>", "</a>", "@", "@1", "<pre></pre>"]
    {
        let got = markdown(&bodies(text), java());
        assert!(got.iter().all(|l| !l.contains(char::is_control)), "{text:?}: {got:?}");
    }
}
