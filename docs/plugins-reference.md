# tarae's plugin API — reference

tarae's plugin API. A plugin is a core WebAssembly module built from this with wit-bindgen, for wasm32-unknown-unknown: it gets no imports — it reads what tarae hands it and answers with actions, and what it asked for (a program's output, a connection's bytes, a file) comes back as events. Positions are byte offsets into the buffer, on character boundaries. Until 1.0 any 0.x version may break plugins: tarae loads only plugins built for its own.

The package is `tarae:plugin@0.3.0`. This is generated from `wit/tarae.wit`, with its doc comments; a plugin's commands, events, permissions and answers are all here.

## What a plugin exports

tarae calls these; a plugin answers with actions.

| export | signature | |
|---|---|---|
| <a id="export-commands"></a>`commands` | `func() -> list<`[`command`](#command)`>` | Called when the plugin loads. |
| <a id="export-subscriptions"></a>`subscriptions` | `func() -> list<`[`subscription`](#subscription)`>` | Called when the plugin loads: the events it wants (none is fine). |
| <a id="export-permissions"></a>`permissions` | `func() -> list<`[`permission`](#permission)`>` | Called when the plugin loads: what it may do beyond the buffer (none is fine — most plugins). |
| <a id="export-run"></a>`run` | `func(name: string, cx: `[`context`](#context)`) -> list<`[`action`](#action)`>` | One of its commands, run by you: its name, and the buffer in front (`buffer`). |
| <a id="export-on"></a>`on` | `func(event: `[`event`](#event)`, cx: `[`context`](#context)`) -> list<`[`action`](#action)`>` | An event it subscribed to, about the buffer in `cx`. |
| <a id="export-on-events"></a>`on-events` | `func(events: list<`[`host-event`](#host-event)`>, now: `[`now`](#now)`) -> list<`[`action`](#action)`>` | What came back from what it asked for, in order. |

## Types

### command

*record* — A command to register: `:name args`, `":name"` in a keymap, and `doc` in the command palette.

| field | type | |
|---|---|---|
| <a id="command-name"></a>`name` | `string` |  |
| <a id="command-doc"></a>`doc` | `string` |  |
| <a id="command-text"></a>`text` | `bool` | Whether it needs the buffer's whole text (`buffer.text`). Most commands only need the selections. |

### event

*enum* — What happens to a buffer, for plugins that subscribe.

| case | |
|---|---|
| <a id="event-opened"></a>`opened` | Loaded (a big file: once its text is in). |
| <a id="event-saved"></a>`saved` | Written to disk. |
| <a id="event-closed"></a>`closed` | About to be closed (its edits can't apply any more). |
| <a id="event-changed"></a>`changed` | Edited, then left alone for a moment (not on every key). |
| <a id="event-before-save"></a>`before-save` | About to be written by `:w` (not auto-save): edits in the answer go into the saved file. Saving waits for the answer, up to 2 seconds. |
| <a id="event-shown"></a>`shown` | Now the buffer in front (switched to, its window focused, opened) — for a view of "this file". The one event a buffer it isn't given has: it comes with an empty buffer, so the view can clear. |

### subscription

*record* — An event to be told about: `text` hands the buffer's whole text with it (`buffer.text`); only about buffers in these languages ("yaml", …; none: every buffer). Only buffers its grant covers have events (`buffer`).

| field | type | |
|---|---|---|
| <a id="subscription-event"></a>`event` | [`event`](#event) |  |
| <a id="subscription-text"></a>`text` | `bool` |  |
| <a id="subscription-languages"></a>`languages` | `list<string>` |  |

### permission

*variant* — What a plugin may do beyond the buffer. It says so in `permissions()`; you approve the list when it's first loaded (and again if it changes), and it can do nothing else.

| case | carries | |
|---|---|---|
| <a id="permission-run"></a>`run` | `string` | Start this program (`spawn`). |
| <a id="permission-read"></a>`read` | `string` | Read files matching this pattern (`read-file`): `~` is your home folder, `*` any name, `**` any folders. Buffers of those files are given to it on its own too (`buffer`). Never a file named like a key (`.env`, `*.pem`, `id_rsa`, …), whatever the pattern says. |
| <a id="permission-env"></a>`env` | `string` | Read this environment variable (`read-env`). |
| <a id="permission-network"></a>`network` |  | Connect over TCP (`connect`); each new host:port is asked about the first time. |
| <a id="permission-project"></a>`project` |  | Read the files of the project you opened (the folder tarae started in): `read-file` there without asking, `find`, and their buffers given to it on its own (`buffer`) — never one named like a key. |
| <a id="permission-keys"></a>`keys` |  | Hear the keys you press as commands (`keys` events) — only while your own config's `plugins.keys` is on, said on the status line while it hears. Never a character typed. A plugin that asks for it asks for nothing else — with `network`, `run`, `read`, `env` or `project` it doesn't load — and what it hears stays in tarae: it answers with its `overlay`, `timer` and `stop` only — every other action, a read too, is refused. |

### selection

*record*

| field | type | |
|---|---|---|
| <a id="selection-anchor"></a>`anchor` | `u64` |  |
| <a id="selection-head"></a>`head` | `u64` |  |
| <a id="selection-from"></a>`from` | `u64` |  |
| <a id="selection-to"></a>`to` | `u64` |  |
| <a id="selection-text"></a>`text` | `string` |  |
| <a id="selection-line"></a>`line` | `u64` | The line `head` is on, from 0. |

### buffer

*record* — The buffer a call is about. A command you run gets the one in front, whatever file it is. On its own (an event, what came back from what it asked for) a plugin gets only a buffer its grant covers — a file its `read` patterns or `project` reach once you've approved them, or a view it shows itself; any other comes as nothing (no path, name, text or selections), and edits and moves there are refused. Never given, even to your command (then refused, said): a file named like a key, or one of tarae's own views.

| field | type | |
|---|---|---|
| <a id="buffer-path"></a>`path` | `option<string>` |  |
| <a id="buffer-name"></a>`name` | `string` |  |
| <a id="buffer-language"></a>`language` | `option<string>` |  |
| <a id="buffer-text"></a>`text` | `option<string>` | The whole text — for a command registered with `text: true`, or an event subscribed with it, on a buffer it's given; none otherwise. |
| <a id="buffer-version"></a>`version` | `u64` |  |
| <a id="buffer-modified"></a>`modified` | `bool` |  |

### now

*record* — What every call gets: the time, and fresh randomness (a plugin has no clock and no random source).

| field | type | |
|---|---|---|
| <a id="now-millis"></a>`millis` | `u64` | Milliseconds since 1970-01-01 UTC. |
| <a id="now-entropy"></a>`entropy` | `list<u8>` | 32 random bytes — enough to seed a cryptographic generator. |

### context

*record*

| field | type | |
|---|---|---|
| <a id="context-args"></a>`args` | `string` |  |
| <a id="context-buffer"></a>`buffer` | [`buffer`](#buffer) |  |
| <a id="context-selections"></a>`selections` | `list<`[`selection`](#selection)`>` |  |
| <a id="context-primary"></a>`primary` | `u64` |  |
| <a id="context-now"></a>`now` | [`now`](#now) |  |

### edit

*record*

| field | type | |
|---|---|---|
| <a id="edit-from"></a>`from` | `u64` |  |
| <a id="edit-to"></a>`to` | `u64` |  |
| <a id="edit-insert"></a>`insert` | `string` |  |

### spawn

*record* — A program to run, with its arguments — no shell. What it prints comes back (`lines`), some lines at a time, then its end (`exit`). Ids are yours, to tell requests apart; one still in use is replaced.

| field | type | |
|---|---|---|
| <a id="spawn-id"></a>`id` | `u64` |  |
| <a id="spawn-program"></a>`program` | `string` |  |
| <a id="spawn-args"></a>`args` | `list<string>` |  |
| <a id="spawn-env"></a>`env` | `list<tuple<string, string>>` | Variables to set for it (`AWS_PROFILE`, …) — only ones the plugin may read (`env` permissions). |

### connect

*record* — A TCP connection: `connected`, then `data` as it comes, `closed` at its end.

| field | type | |
|---|---|---|
| <a id="connect-id"></a>`id` | `u64` |  |
| <a id="connect-host"></a>`host` | `string` |  |
| <a id="connect-port"></a>`port` | `u64` |  |

### panel

*record* — The plugin's panel, below the code: a table. You move through its rows with tarae's moving keys, as in every list panel — j k ↓ ↑ C-n C-p, C-e C-y, C-d C-u, C-f C-b, the page keys, g G, Home End — and space, : and C-w stay the editor's, esc gives the keys back: those never come to the plugin. Other keys do (`key`).

| field | type | |
|---|---|---|
| <a id="panel-title"></a>`title` | `string` |  |
| <a id="panel-columns"></a>`columns` | `list<string>` | Column names (none: no header row). |
| <a id="panel-rows"></a>`rows` | `list<list<string>>` | A cell per column. |
| <a id="panel-ids"></a>`ids` | `list<string>` | An id per row ("namespace/name"), or none. With ids, the selection stays on its row as rows come, go and move, and a key says which row it was on. |
| <a id="panel-status"></a>`status` | `string` | A line under the table. |
| <a id="panel-keys"></a>`keys` | `list<tuple<string, string>>` | Keys to show in the header, with what they do: ("d", "describe"). |

### view

*record* — A read-only buffer the plugin fills (`show`, then `append`): a describe, a YAML, a log.

| field | type | |
|---|---|---|
| <a id="view-id"></a>`id` | `u64` |  |
| <a id="view-title"></a>`title` | `string` |  |
| <a id="view-text"></a>`text` | `string` |  |
| <a id="view-language"></a>`language` | `option<string>` | For colors: "yaml", "json", … (none: plain). |

### tone

*enum* — How a hint or a card row reads: fine, of note, a warning, an error.

| case | |
|---|---|
| <a id="tone-ok"></a>`ok` |  |
| <a id="tone-info"></a>`info` |  |
| <a id="tone-warning"></a>`warning` |  |
| <a id="tone-error"></a>`error` |  |

### hint

*record* — Text at the end of a buffer's line, beside the code (from 0).

| field | type | |
|---|---|---|
| <a id="hint-line"></a>`line` | `u64` |  |
| <a id="hint-text"></a>`text` | `string` |  |
| <a id="hint-tone"></a>`tone` | [`tone`](#tone) |  |

### hints

*record* — A plugin's hints for one buffer, worked out on its text at `version` (a buffer's `version`): they replace what it put there before, and show only while the buffer is at that version — edited, they go until the plugin sends them again (it hears `changed`).

| field | type | |
|---|---|---|
| <a id="hints-path"></a>`path` | `string` |  |
| <a id="hints-version"></a>`version` | `u64` |  |
| <a id="hints-hints"></a>`hints` | `list<`[`hint`](#hint)`>` |  |

### side-row

*record* — A row of its side tab — a tree, by depth.

| field | type | |
|---|---|---|
| <a id="side-row-id"></a>`id` | `string` |  |
| <a id="side-row-depth"></a>`depth` | `u64` |  |
| <a id="side-row-text"></a>`text` | `string` |  |
| <a id="side-row-detail"></a>`detail` | `string` | Dim, at the right. |
| <a id="side-row-tone"></a>`tone` | [`tone`](#tone) |  |
| <a id="side-row-at"></a>`at` | `option<tuple<string, u64, u64>>` | Where it's defined: (path, first line, last line), from 0. `enter` goes there — only to a file it may read (`read`, `project`) —, and while the cursor is there it's the row marked. None: `enter` comes to the plugin (`key`). |
| <a id="side-row-note"></a>`note` | `bool` | A line of text under the row above it (why that one isn't fine): in its tone's color, with no glyph, not a row to select. |

### side

*record* — Its tab in the side panel, beside files, outline and git.

| field | type | |
|---|---|---|
| <a id="side-title"></a>`title` | `string` | The tab's name. |
| <a id="side-note"></a>`note` | `string` | At the right of the panel's header: what it's of. |
| <a id="side-rows"></a>`rows` | `list<`[`side-row`](#side-row)`>` |  |
| <a id="side-keys"></a>`keys` | `list<tuple<string, string>>` | Its keys, for the panel's foot: ("e", "events"). Keys the panel doesn't take itself come to the plugin (`key`, with the row's id) — never tarae's moving keys (j k ↓ ↑ C-n C-p, C-e C-y, C-d C-u, C-f C-b, the page keys, g G, Home End), folding (h l), enter, tab, esc, q, space, : or C-w. |
| <a id="side-open"></a>`open` | `bool` | Show the side panel at this tab now — without the keys, or (`focus`) with them. |
| <a id="side-focus"></a>`focus` | `bool` |  |

### role

*enum* — How a span reads — a role, never a colour: the theme gives each its colour.

| case | |
|---|---|
| <a id="role-text"></a>`text` | Full ink: what's there to be read. |
| <a id="role-faint"></a>`faint` | Dim: what goes with it. |
| <a id="role-key"></a>`key` | A keycap: the text on the selected row's ground, a cell of it each side. |

### span

*record*

| field | type | |
|---|---|---|
| <a id="span-text"></a>`text` | `string` |  |
| <a id="span-role"></a>`role` | [`role`](#role) |  |

### overlay

*record* — The plugin's overlay: a few rows floating over the screen, borderless — news, never taking a key. It shows at once and stays `hold` milliseconds (none: the time its text takes to read), then fades out; shown again, it's replaced (a row that changed lands lit and dries). At most 3 rows of 80 cells (a newline is a space, control characters are replaced). While it's up it's never narrower than it was. It floats bottom-right, where tarae's asks stand: not drawn while one does, nor while which-key is up — back when the corner is empty. Never over a card, a toast, the cursor's row or the change in hand: it moves a few rows up out of their way, else it isn't drawn until they go; never under a picker or a prompt.

| field | type | |
|---|---|---|
| <a id="overlay-rows"></a>`rows` | `list<list<`[`span`](#span)`>>` |  |
| <a id="overlay-hold"></a>`hold` | `option<u64>` |  |

### mode

*enum* — The editor's mode after a key.

| case | |
|---|---|
| <a id="mode-normal"></a>`normal` |  |
| <a id="mode-insert"></a>`insert` |  |
| <a id="mode-select"></a>`select` |  |

### key-event

*record* — A key you pressed, as a command — never a character typed: in insert mode only the key that left it, in a line being typed (a prompt, a picker, the chat) only `enter` and `esc`, nothing the terminal's program gets, and never the character a command waits for (`f x`, `r x`, `"a`: only `f`, `r`, `"`).

| field | type | |
|---|---|---|
| <a id="key-event-keys"></a>`keys` | `list<string>` | The sequence so far, as tarae's hints name its keys: ["space", "f"], ["C-w", "h"], ["3", "j"], ["esc"], ["enter"], ["-"], ["↑"] — the names tarae shows, not the keymap's notation `key` uses. Empty when insert mode was left by a binding of characters (`j k`): none is named. |
| <a id="key-event-done"></a>`done` | `bool` | False: a sequence under way (more keys to come; which-key may be up). |
| <a id="key-event-mode"></a>`mode` | [`mode`](#mode) |  |

### pick

*record* — A list to choose from (tarae's picker): the choice comes back as `answer` (its id, or none: esc).

| field | type | |
|---|---|---|
| <a id="pick-id"></a>`id` | `u64` |  |
| <a id="pick-title"></a>`title` | `string` |  |
| <a id="pick-items"></a>`items` | `list<tuple<string, string>>` | (id, what it shows) |

### action

*variant* — What tarae does with an answer, in order. The edits of one answer (replace-selections, edits, select) are one undo step, on the buffer and selections the command was given, and are refused if the buffer changed while the plugin ran or is being typed in. Text is shown without control characters, and an answer may hold up to 64 MB of text and lists. What needs a permission fails (`failed`) without one.

| case | carries | |
|---|---|---|
| <a id="action-status"></a>`status` | `string` |  |
| <a id="action-success"></a>`success` | `string` |  |
| <a id="action-warning"></a>`warning` | `string` |  |
| <a id="action-error"></a>`error` | `string` |  |
| <a id="action-replace-selections"></a>`replace-selections` | `list<string>` | One text per selection. |
| <a id="action-edits"></a>`edits` | `list<`[`edit`](#edit)`>` |  |
| <a id="action-select"></a>`select` | `list<tuple<u64, u64>>` | (anchor, head) per selection, in the buffer after this answer's edits; the first is the primary one. |
| <a id="action-run"></a>`run` | `string` | A tarae movement command (`move_next_word_start`, `goto_file_start`, …). Nothing else runs: not typed commands, not other commands. |
| <a id="action-open"></a>`open` | `string` | Opens a file — at a line, as `path:line` (from 1) — only one it may read (`read`, `project`): any other is refused, said. |
| <a id="action-status-bar"></a>`status-bar` | `string` | A short text in the status line, for this buffer (while it's in front), until replaced; an empty one clears it. |
| <a id="action-spawn"></a>`spawn` | [`spawn`](#spawn) | Starts a program (`run` permission). |
| <a id="action-connect"></a>`connect` | [`connect`](#connect) | Opens a TCP connection (`network` permission). |
| <a id="action-send"></a>`send` | `tuple<u64, list<u8>>` | Sends bytes on a connection. |
| <a id="action-stop"></a>`stop` | `u64` | Stops what an id started: a program, a connection, a timer. |
| <a id="action-read-file"></a>`read-file` | `tuple<u64, string>` | Reads a whole file (`read` permission): (id, path). |
| <a id="action-read-env"></a>`read-env` | `tuple<u64, string>` | Reads an environment variable (`env` permission): (id, name). |
| <a id="action-timer"></a>`timer` | `tuple<u64, u64>` | A `tick` after this many milliseconds: (id, ms). |
| <a id="action-panel"></a>`panel` | `option<`[`panel`](#panel)`>` | Shows its panel, replacing what it showed — or closes it. |
| <a id="action-prompt"></a>`prompt` | `tuple<u64, string>` | Asks for a line of text below the panel: (id, prompt). The answer comes back (`answer`). Only in answer to you — a command, a key in its panel, its last prompt — and not while a line is being typed (then it fails). |
| <a id="action-show"></a>`show` | [`view`](#view) | Opens (or replaces) a read-only buffer. |
| <a id="action-append"></a>`append` | `tuple<u64, string>` | Adds text to the end of a buffer it showed: (id, text). When you close that buffer, the plugin is told (`failed`, with its id), and appending to it fails (it stays closed). |
| <a id="action-hints"></a>`hints` | [`hints`](#hints) | Text beside a buffer's lines. |
| <a id="action-side"></a>`side` | `option<`[`side`](#side)`>` | Its tab in the side panel (replacing it) — or none. |
| <a id="action-pick"></a>`pick` | [`pick`](#pick) | Asks you to choose — only in answer to you, as `prompt`. |
| <a id="action-badge"></a>`badge` | `option<tuple<string, `[`tone`](#tone)`>>` | A short text in the status line on every buffer (what it's connected to), in a tone — or none. |
| <a id="action-find"></a>`find` | `tuple<u64, string>` | The project's files a pattern matches ("**/kustomization.yaml", from the project's folder; `project` permission): (id, pattern). They come back (`found`), as absolute paths. |
| <a id="action-write-own"></a>`write-own` | `tuple<u64, string, list<u8>>` | Writes a file of its own (in its own folder, under tarae's state folder — nowhere else, and no permission needed): (id, name, bytes). Where it is comes back (`written`). |
| <a id="action-offer-install"></a>`offer-install` | `tuple<u64, string>` | Offers to install a tool the language packs know how to (`kustomize`): a card you answer. Once it's in, `installed`; if the install fails, `failed`. Once per tool until tarae quits. |
| <a id="action-overlay"></a>`overlay` | `option<`[`overlay`](#overlay)`>` | Its overlay (replacing the one it showed) — or none: gone at once. |

### host-event

*variant* — What comes back from what a plugin asked for.

| case | carries | |
|---|---|---|
| <a id="host-event-lines"></a>`lines` | `tuple<u64, list<string>>` | A program's lines on stdout, in order (without their newlines). |
| <a id="host-event-exit"></a>`exit` | `tuple<u64, option<u64>, list<string>>` | A program ended: its exit code (none if a signal stopped it), and its last lines on stderr. |
| <a id="host-event-connected"></a>`connected` | `u64` | A connection is open. |
| <a id="host-event-data"></a>`data` | `tuple<u64, list<u8>>` | Bytes from a connection. |
| <a id="host-event-closed"></a>`closed` | `tuple<u64, string>` | A connection closed: why (empty when the other end just closed it). |
| <a id="host-event-file"></a>`file` | `tuple<u64, list<u8>>` | A file's bytes. |
| <a id="host-event-env"></a>`env` | `tuple<u64, option<string>>` | An environment variable's value (none: not set). |
| <a id="host-event-tick"></a>`tick` | `u64` | A timer went off. |
| <a id="host-event-key"></a>`key` | `tuple<string, option<u64>, option<string>>` | A key in its panel or its side tab (tarae's name for it: "d", "ret", "C-k", …), the row selected then, and that row's id (if it has ids). Never one of tarae's moving keys (`panel`'s note). |
| <a id="host-event-answer"></a>`answer` | `tuple<u64, option<string>>` | What was typed at its prompt, or chosen in its pick (none: esc). |
| <a id="host-event-failed"></a>`failed` | `tuple<u64, string>` | A request couldn't be done: why (no permission, no such file, refused, …). |
| <a id="host-event-found"></a>`found` | `tuple<u64, list<string>>` | What `find` found. |
| <a id="host-event-written"></a>`written` | `tuple<u64, string>` | Where `write-own` wrote. |
| <a id="host-event-installed"></a>`installed` | `u64` | A tool it offered is installed. |
| <a id="host-event-pressed"></a>`pressed` | `u64` | A press on its overlay: the row pressed, from 0. |
| <a id="host-event-keys"></a>`keys` | `list<`[`key-event`](#key-event)`>` | Keys you pressed, in order (`keys` permission): one call for those that came while it was busy. |

## Examples

Whole plugins, one idea each, with their tests ([`plugin-sdk/examples`](../plugin-sdk/examples)); `tarae plugin new <dir> --from <example>` starts a crate from one. The guide is [plugins.md](plugins.md).

- [`sort-lines`](../plugin-sdk/examples/sort-lines/src/lib.rs) — a command over the selections
- [`yaml-tabs`](../plugin-sdk/examples/yaml-tabs/src/lib.rs) — an event with a language filter
- [`todo-hints`](../plugin-sdk/examples/todo-hints/src/lib.rs) — hints tied to a buffer version
- [`word-tally`](../plugin-sdk/examples/word-tally/src/lib.rs) — a panel with its own keys
- [`md-outline`](../plugin-sdk/examples/md-outline/src/lib.rs) — a side tab
- [`read-notes`](../plugin-sdk/examples/read-notes/src/lib.rs) — reading a file under a `read` pattern
- [`git-branch`](../plugin-sdk/examples/git-branch/src/lib.rs) — a program run with `run(prog)`
- [`http-status`](../plugin-sdk/examples/http-status/src/lib.rs) — an HTTP call through the SDK
- [`final-newline`](../plugin-sdk/examples/final-newline/src/lib.rs) — a before-save check
- [`stamps`](../plugin-sdk/examples/stamps/src/lib.rs) — state kept with `write-own`

<!-- docs/plugins-reference.md: generated by tarae (src/plugin/reference.rs) from wit/tarae.wit — don't edit by hand -->
