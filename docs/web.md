# Web pages

A page is a buffer, as code is: read-only, in the pane you're in, listed in `space b`, closed with `:bc`, read beside
the code with `C-w v`. Its links answer the keys code answers — `gd` follows one, `space k` says where it goes — so
reading the docs of what you're writing asks nothing new of you. tarae shows text, never runs a page: no JavaScript, no
cookies, GET only.

## Opening a page

- **`:web https://… [main|article|page]`** fetches a page and reads it as markdown. The buffer is made at once
  (`web · docs.rs`), its line `reading docs.rs…  3.2s  C-c stop`; it fills as the page lands (`web · docs.rs › Rope
  in ropey`, its line `main · 87k chars · 2m ago`), how it read on the status line (`main · 87k chars · 753 links ·
  …`). `C-c` on the page stops its read; closing the buffer does too; leaving it doesn't. A read that fails says its
  short why on the line (`nothing answers there`, in warning) and the whole of it on the page's first row
  (`http://localhost:3000/: nothing answers there · connection refused · os error 61`).
- **`gf`** on an address anywhere — in code, a comment, a page — opens it as a page. On a path it opens the file as
  `:open` does (from the file's folder, else the project's; `file:12:5` at its line), on a folder its file list.
  Several selections open each, the last in front (`C-o` goes back through them).
- **A link's `gd`** opens the page it goes to (below).
- **`:web <path> [address] [main|article|page]`** reads a page saved as HTML, `address` where it came from (its
  links resolve against it). Into the buffer of that address the file is its source from then on: `:reload` and
  `:web-reader` read the file again, `:web!` fetches.
- **Other words** — `:web tokio set_times`, or a word that names no file — get one line: `:web takes an address
  (https://…) or a saved page, not other words`.

A buffer is one copy. `:web` of an address already open shows that buffer; **`:reload`** (or `:web!`) fetches it anew
into it. A whole page fetched is the session's for 15 minutes (`read 2m ago`) — never an error's page, one maybe cut
short or this machine's — so a page closed and opened again isn't fetched twice. A redirect to an address another
buffer has shows that one: a buffer that never landed goes, one read before keeps what it showed (said).

## Reading

In markdown: the page's `<main>` when it has one, else its article (a lone `<article>`, else the block its title
heads or its text is densest in), else the whole page — the site's chrome, what it runs and what it hides left out;
each link where it stands, each code block fenced with its language. **`:web-reader main|article|page`** reads the page
in sight again as another part, from its copy (nothing fetched). A page that needs JavaScript says so, and
**`:web-open`** hands its address — and nothing else — to your browser.

The page is read-only: an edit is put back and said. It's copied as code is (select, `y`, `space >` to quote it to
Claude), and **`:w <path>`** saves a copy of its text to a file of yours, its first line an HTML comment saying where it
came from and when (`<!-- https://docs.rs/… · read 2026-10-23 14:02 -->`); `:w` alone says so.

## Its keys

A page answers the code's language keys itself, in tarae — no server process (`:procs` lists none):

| Key | On a page | In code |
|---|---|---|
| `gd` | the link's target: another page (its buffer, or fetched), an `#anchor` a move in the page | the definition |
| `gr` | on a heading the links to it; on a link every link to the same address | references |
| `space k` | where the link goes — another host, open or kept with its age | hover |
| `space s` | the headings, as a tree | the file's symbols |
| `space o` | the headings (markdown's outline) | the outline |
| `C-o` / `C-i` | the jump list, across pages and code alike | the same |
| `gf` | the address under the cursor, as a page | the path or address under the cursor |
| `ctrl`+click | `gd` there | `gd` there; on an http(s) address written in the code, `gf` (it opens as a page) |
| `C-c` | stop the page's read | `toggle_comments` |

Only `http(s)` links are followed: a `mailto:`, `file://` or `javascript:` link's `gd` says so and opens nothing.
`[lsp.web] except = ["hover"]` turns a key off as for any server.

A click puts the cursor on the character under it; **`ctrl`+click goes there, inside tarae** — `gd`, or on an
address written in code (a comment, a string) `gf`: the page opens here (the system browser is `:web-open`). The cursor
is put there first, so `C-o` comes back. Where the terminal doesn't send `ctrl` with a click (macOS's Terminal.app and iTerm2 can make ctrl+click a
right click) it's a plain click, and nothing can tell the two apart: the first plain click on a link's words in a
session says `gd` follows it.

## The fetch

tarae's own HTTP/1.1 GET on a worker, through the same egress proxy as Claude's commands — no card for yours (the
address you typed, or the link you followed, is the yes) but every address refusal: never a cloud metadata server,
link-local, this machine's own addresses; your upstream proxy as theirs. TLS by rustls, verified as your system
verifies (a company's own root works). `http://` is read over https but on this machine (`localhost`, reached
directly); redirects followed, five at most, another host's said (`from github.com`); gzip unpacked. 20 s to the last
byte. A user or password in the address, a bare IP address and a host name that isn't ASCII are refused, said.

**Nothing of yours is sent**: no cookie, credentials or referrer — but your wiki's token at its base (below). A page in
front is named to Claude by its address
(`file: https://… (web page, read-only; its text isn't sent — read takes its address)`), never its text — a page is
someone else's words, and Claude gets them only framed as such, by its own `read`.

## Claude reads pages

Claude's `read` takes an `https://` URL beside files: a page over 16 KB comes as its outline (headings with their
lines, links out), then `url:120-200` or `url#heading` its part; under it, whole. A host Claude's commands don't reach
asks on a card first (`y` this read · `a` this chat · `s` this session · `n`) — a read's yes never lets a command
reach it (a command can post and run what it downloads), while a command's yes lets Claude read there; the fetch starts
after your answer (30 s to the last byte), through the same proxy and address refusals as yours, another host's
redirect said, not followed. The page goes to Claude framed as someone else's words, never instructions, lines that
look like keys hidden; the session's copy is shared with `:web` (no second fetch). `:granted` lists the hosts, `d`
takes one back.

**Follow and replay**: each page read is a replay stop. While follow leads, Claude's read shows as a file read does —
the copy it read, at the lines it read, its line saying `the copy Claude read 2m ago`; paused, nothing opens.
`]r`/`[r`, `C-]` and the chat's row show it later. A copy is never fetched again: one the session no longer keeps is
said. This is the one place follow goes outside the project — a page fetched through the gate you answered, opening no
file and running nothing.

## Your work wiki

A wiki you name in your own config (a project's file never does) is read with your token:

```toml
[web]
signed-in = [{ base = "https://corp.example/confluence", kind = "confluence-dc", token-env = "CONFLUENCE_PAT" }]
```

tarae reads the token from that variable of its own environment and sends it as `Authorization: Bearer` with its
own GET only: over https, to the base's host and port, at or below its path — judged for each request, so a redirect
elsewhere goes on without it. Unset, a read there says so (`CONFLUENCE_PAT isn't set in tarae's environment — export
it and start tarae again`); a 401 says the token was refused (expired or revoked?), a 403 that your account isn't
allowed this with it — never the sign-in page. The variable never
reaches Claude's commands or any claude tarae starts. Confluence Data Center's page addresses — `/display/SPACE/Title`,
a blog post's `/display/SPACE/yyyy/mm/dd/Title`, `/pages/viewpage.action?pageId=…`, a short link `/x/…` — are read
through its REST API (`runtime/web/sources.toml`) and come at their own address, so links between pages and `:web-open`
work as on any site; its search page (`/dosearchsite.action?queryString=…`) reads as a list of pages. A chat started
with a wiki named is told where it is and how its search reads, in one line.

Claude's read of a page there asks on a card of its own (`Let Claude read a page at corp.example with your
token?`); its `a` or `s` holds for later reads there, as any read's does. Nothing else opens it: not a command's
grant, `llm.sandbox-hosts`, an answer on a plain read's card at that host, the copy `:web` fetched, or a redirect
from a plain read into the base (said to Claude, its next read asked). With the variable unset when tarae started,
the read is refused before any card. Once a chat read a page with the token, each of its web searches asks (`y` this
search · `n` not this one), continued or restarted too, since what it read could leave in the words. `:granted` lists
the wiki with the variable its token comes from, never the token.

## Claude searches the web

A chat has a `web_search` tool: web search is on by default, and a chat's first search asks you on a card.
`llm.web-search = false` in your own config removes the tool from the chats started after it (a project's file never
sets it, either way). Claude gives it words (and, if it likes, sites: `docs.rs github.com`); tarae starts a
small `claude -p` beside the chat on `llm.web-search-model` (`haiku`), told only those words, in an empty folder of its
own, with Claude Code's WebSearch its one tool. What comes back to the chat is a list, not a summary: up to ten pages,
title and address, framed as someone else's words (no dates: the search engine gives none). Claude reads a page of it
with `read`, whose host card is as it was. The tool tells Claude it's for a page it can't name (an error's words, a tool
it doesn't know): an address it knows or can guess (a project's releases, its issues' own search, a registry) it reads
instead.

A chat's first search asks on a card that shows its words, since they leave the machine for Anthropic's search engine:
`y` this chat · `s` this session · `n` no, for the chat. After an answer that chat's searches ask nothing. A query that
looks like it holds a key is refused before anything starts. What a search cost is an entry of its own in `:usage`
(`web-search`), billed as the chat is, never part of a chat turn's cost. Each search is a replay stop: follow and `]r`
show the list Claude got, as a page whose links `gd` follows. The turn's end stops a search still under way.

`:usage`'s tool calls count, a turn, the pages `read` asked for and didn't bring (an address that isn't there, a host
refused) and the web searches made — with `llm.web-search` on or off — so what search saves, and what reading
first costs, is seen from use.

## Limits

Read on a worker by memory-safe code, within limits — 8 MB of page, 512 levels of nesting, 2 MB of text, each said
when hit. The page's own keys answer from what the reader found (at most 2,000 headings and 1,000 places an answer).
