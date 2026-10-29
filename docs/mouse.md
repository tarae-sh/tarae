# The mouse

What each act does on each surface — printed from `mouse::expect` (`src/mouse.rs`), the table the walk
(`mouse::tests::no_press_is_lost_in_silence`) holds every scene to; `TARAE_BLESS=1 cargo test mouse` writes it
again. The rules (CLAUDE.md, "The mouse, one way on every surface"): a press gives the keys to the surface it
lands on; a drag selects wherever there's text; the wheel scrolls what's under the pointer and nothing else;
every line between two things with sizes drags; a word that names a state does its key. A press, a drag or a
wheel that does nothing says why — nothing at all only for a hover, a release, a wheel with nothing to scroll.

✓ acts · says: it says why not · — nothing, by design · ✗ a gap, and the landing of `.sb/mouse-plan.md` §4
that fills it (S selection).

| Surface | Part | Press | Release | Drag | Double | Wheel | Hover |
|---|---|---|---|---|---|---|---|
| The code | text | ✓ | — ctrl+press goes there at the release | ✓ | ✓ | ✓ | — no hover from the language server — K |
| The code | a presence tag | ✓ | — ctrl+press goes there at the release | ✓ | as one press | ✓ | ✓ |
| A pane's top line |  | ✓ | — the press did it | — tiled panes don't move | as one press | ✓ | — its line between the names: nothing to press |
| A pane's top line | a name in its path (its row in the side panel's files), a buffer's name, a key hint (`enter go there`) | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | ✓ |
| A line between panels, between your panes | the chat's, the side panel's, the rail's, the slot's, a dock's, a divider | ✓ | ✓ | ✓ | as one press | — nothing to scroll | ✓ |
| Claude's edit review | a change | ✓ | — the press did it | ✓ | as one press | ✓ | — nothing on hover |
| Claude's edit review | off it | says: the review waits for its answer — y accepts the change, n rejects it | — the press did it | says: the review waits for its answer — y accepts the change, n rejects it | as one press | — nothing to scroll | — nothing on hover |
| The review's rail | a file | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| The review's rail | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| The review's rail | a key hint at its foot | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | — nothing on hover |
| The chat's conversation | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — a cut row comes whole under it |
| The chat's conversation | a link, a tool row, an id | ✓ | — the press did it | ✗ S: a drag from a link | as one press | ✓ | — a cut row comes whole under it |
| The chat's input | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — a cut row comes whole under it |
| The chat | its top line, its gaps | ✓ | — the press did it | — tiled panes don't move — its line toward the code sizes it | as one press | ✓ | — nothing on hover |
| The chat | a session's name, `+N` | ✓ | — the press did it | — tiled panes don't move — its line toward the code sizes it | as one press | ✓ | ✓ |
| The chat | what goes with the message over its input (a quote, `screen`, an element pointed at, `N things`) | ✓ | — the press did it | — tiled panes don't move — its line toward the code sizes it | as one press | ✓ | ✓ |
| The chat | `follow`, `paused`, the turn's word, the model, a key hint at its foot | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | ✓ |
| The follow card | a trail row, the card | ✓ | — the press did it | — placed by rule | as one press | — nothing to scroll | — nothing on hover |
| The follow card | a strip mark | ✓ | — the press did it | — placed by rule | as one press | — nothing to scroll | ✓ |
| The follow card | `▲ waits`, `paused`, the turn's word, its keys | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | — nothing to scroll | ✓ |
| The side panel | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | — a row opens on one press — the git tab's opens on two | ✓ | — nothing on hover |
| The side panel | a tab's name on its top line, a key hint at its foot | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | ✓ |
| The side panel | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| Run output | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — nothing on hover |
| Run output | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer | as one press | ✓ | — nothing on hover |
| Run output | a preflight's step chip | ✓ | — the press did it | ✗ S: a drag follows the pointer | as one press | ✓ | ✓ |
| A group's list | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✗ S: again opens, as the session list's | ✓ | — nothing on hover |
| The slot's line | a tab, a member's name | ✓ | — the press did it | ✓ | as one press | — nothing to scroll | ✓ |
| The slot's line | its keys | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | — nothing to scroll | ✓ |
| The terminal |  | ✓ | ✓ | ✓ | ✗ S: a word | ✓ | — nothing on hover |
| The terminal's view mode |  | ✓ | — the press did it | ✓ | ✓ | ✓ | — nothing on hover |
| A plugin's panel |  | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A floating list (sessions, `:messages`) | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| A floating list (sessions, `:messages`) | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A floating pane | its frame | ✓ | — a click keeps nothing; a drag's release keeps it | ✓ | as one press | ✓ | — nothing on hover |
| A floating pane | inside | ✓ | — the press did it | ✗ S: its text selects | as one press | ✓ | — nothing on hover |
| A floating pane | a key hint on its lines | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | ✓ |
| Cards | an answer, the card | ✓ | — the press did it | says: a card's text isn't selected — v shows it whole, to copy | as one press | ✓ | ✓ |
| The ask's card (`space i`, a commit draft) |  | ✓ | — the press did it | says: the ask's card isn't selected — its change comes to the buffer, reviewed there | as one press | — nothing to scroll | — nothing on hover |
| The ask's card (`space i`, a commit draft) | a key hint | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | — nothing to scroll | — nothing on hover |
| The notification card |  | ✓ | — the press did it | — the press opened :messages | — the first press opened :messages | — news isn't scrolled | — nothing on hover |
| A plugin's overlay | a row | ✓ | — the press did it | — what's under it takes it | as one press | — what's under it takes it | — what's under it takes it |
| A picker | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| A picker | its query | ✓ | — the press did it | ✓ | as one press | ✓ | — nothing on hover |
| A picker | its frame's other rows | — no row there | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A picker | a key hint on its frame | — no row there | — the press did it | says: this takes a press — a drag starts beside it | as one press | ✓ | — nothing on hover |
| A picker | outside it | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | — the first press closed it | ✓ | — nothing on hover |
| A picker's preview |  | says: a press on a row of the list picks it | — the press did it | ✗ S: the preview's text selects | as one press | ✓ | — nothing on hover |
| The prompt | a completion | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | — tab moves through its completions | — nothing on hover |
| The prompt | its input | ✓ | — the press did it | ✓ | as one press | — tab moves through its completions | — nothing on hover |
| The prompt | outside it | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | — tab moves through its completions | — nothing on hover |
| Hover text, docs |  | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | ✓ | — only the wheel is its; the rest goes under |
| Which-key | a key's row | ✓ | — the press did it | — its press did it — a drag goes to what's under it | as one press | — nothing to scroll | — nothing on hover |
| Which-key | the rest of it | says: a row presses it | — the press did it | — its press did it — a drag goes to what's under it | as one press | — nothing to scroll | — nothing on hover |
| The completion menu | a row | ✓ | — the press did it | — its press did it — a drag goes to what's under it | as one press | ✓ | — nothing on hover |
| The completion menu | the rest of it, its docs | says: a row presses it | — the press did it | — its press did it — a drag goes to what's under it | as one press | ✓ | — nothing on hover |
| The note card |  | ✓ | — the press did it | — its press did it — a drag goes to what's under it | as one press | — nothing to scroll | — nothing on hover |
| The note card | a key hint | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | — nothing to scroll | — nothing on hover |
| The status line | a word (`▲N waiting · C-q`, the chip, what Claude does now, the branch, `▲ N`, `full screen`) | ✓ | — the press did it | — a line of state, no text to select | as one press | — nothing to scroll | ✓ |
| The status line | a key hint (a list's, a picker's, a prompt's) | ✓ | — the press did it | says: this takes a press — a drag starts beside it | as one press | — nothing to scroll | ✓ |
| The status line | the rest | — no word there: the mode and the position say where the keys are | — the press did it | — a line of state, no text to select | as one press | — nothing to scroll | — nothing on hover |
| The start screen | a row with keys | ✓ | — the press did it | says: the start screen has nothing to select — a press on a row presses its keys | as one press | — nothing to scroll | — nothing on hover |
| The start screen | the rest | says: a row of the start screen presses its keys | — the press did it | says: the start screen has nothing to select — a press on a row presses its keys | as one press | — nothing to scroll | — nothing on hover |
| Pointing's labels |  | ✓ | — the press did it | says: pointing takes a press — a drag picks nothing | as one press | — nothing to scroll | — the press did it |

States that decide a press, on any surface:

| State | Press |
|---|---|
| `C-w m` moving a floating pane: a press on it, not its top line | says: moving this pane — h j k l move it, enter done, its top line drags it |
| a key sequence under way (`space`, `C-w`, `g`, a count…): a press ends it, then acts where it lands — which-key's row presses its key, a word does its key (in normal mode; mid-insert a word's keys are refused, said); insert mode's held keys (a `j k` binding's `j` — the code's, the chat's input's, the prompt's line's) are text: typed where they were typed, never dropped; `C-r` waiting for a register's name, ended — in the code's insert mode and every line alike | ✓ |

The other buttons:

| Button | What it does |
|---|---|
| The middle button | the clipboard pasted at the pointer, the cursor put there first, on text that takes it — the code in normal or insert mode, the chat's input, a prompt's line; anywhere else nothing, said |
| The wheel sideways | the code pane under the pointer, its view's columns; elsewhere nothing to scroll |
| The right button | the terminal's — tarae doesn't take it |
