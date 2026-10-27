# The mouse

What each act does on each surface — printed from `mouse::expect` (`src/mouse.rs`), the table the walk
(`mouse::tests::no_press_is_lost_in_silence`) holds every scene to; `TARAE_BLESS=1 cargo test mouse` writes it
again. The rules (CLAUDE.md, "The mouse, one way on every surface"): a press gives the keys to the surface it
lands on; a drag selects wherever there's text; the wheel scrolls what's under the pointer and nothing else;
every line between two things with sizes drags; a word that names a state does its key. A press, a drag or a
wheel that does nothing says why — nothing at all only for a hover, a release, a wheel with nothing to scroll.

✓ acts · says: it says why not · — nothing, by design · ✗ a gap, and the landing of `.sb/mouse-plan.md` §4
that fills it (S selection, R resize, W words, O overlays and buttons).

| Surface | Part | Press | Release | Drag | Double | Wheel | Hover |
|---|---|---|---|---|---|---|---|
| The code | text | ✓ | — ctrl+press goes there at the release | ✓ | ✓ | ✓ | — no hover from the language server — K |
| The code | a presence tag | ✓ | — ctrl+press goes there at the release | ✓ | as one press | ✓ | — no hover from the language server — K |
| A pane's top line |  | ✓ | — the press did it | — tiled panes don't move | as one press | ✓ | ✗ W: its words' keys on hover |
| A line between panels |  | ✗ R: a press focuses what it heads | — the press did it | ✓ | as one press | — nothing to scroll | ✗ R: it lights under the pointer |
| Claude's edit review | a change | ✓ | — the press did it | ✓ | as one press | ✓ | — nothing on hover |
| Claude's edit review | off it | ✗ O: off the review: nothing until it's answered | — the press did it | ✗ O: off the review: nothing until it's answered | as one press | — nothing to scroll | — nothing on hover |
| The review's rail | a file | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| The review's rail | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| The chat's conversation | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — a cut row comes whole under it |
| The chat's conversation | a link, a tool row, an id | ✓ | — the press did it | ✗ S: a drag from a link | as one press | ✓ | — a cut row comes whole under it |
| The chat's input | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — a cut row comes whole under it |
| The chat | its top line, its gaps | ✓ | — the press did it | ✗ R: its top line, its edge: nothing to drag there | as one press | ✓ | — nothing on hover |
| The chat | a session's name, `+N` | ✓ | — the press did it | ✗ R: its top line, its edge: nothing to drag there | as one press | ✓ | — nothing on hover |
| The follow card | a trail row, the card | ✓ | — the press did it | — placed by rule | as one press | ✗ O: the wheel passes under it | — nothing on hover |
| The follow card | a strip mark | ✓ | — the press did it | — placed by rule | as one press | ✗ O: the wheel passes under it | ✓ |
| The side panel | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | — a row opens on one press — the git tab's opens on two | ✓ | — nothing on hover |
| The side panel | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| Run output | text | ✓ | — the press did it | ✓ | ✓ | ✓ | — nothing on hover |
| Run output | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer | as one press | ✓ | — nothing on hover |
| Run output | a preflight's step chip | ✓ | — the press did it | ✗ S: a drag follows the pointer | as one press | ✓ | — nothing on hover |
| A group's list | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✗ S: again opens, as the session list's | ✓ | — nothing on hover |
| The slot's line | a tab, a member's name | ✓ | — the press did it | ✗ R: the slot's line drags its height | as one press | — nothing to scroll | — nothing on hover |
| The terminal |  | ✓ | ✓ | ✓ | ✗ S: a word | ✓ | — nothing on hover |
| The terminal's view mode |  | ✓ | — the press did it | ✓ | ✓ | ✓ | — nothing on hover |
| A plugin's panel |  | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A floating list (sessions, `:messages`) | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| A floating list (sessions, `:messages`) | off its rows | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A floating pane | its frame | ✓ | — the press did it | ✓ | as one press | ✓ | — nothing on hover |
| A floating pane | inside | ✓ | — the press did it | ✗ S: its text selects | as one press | ✓ | — nothing on hover |
| Cards | an answer, the card | ✓ | — the press did it | ✗ O: says v shows it whole to copy (Q11) | as one press | ✗ O: a cut card's body scrolls | — nothing on hover |
| The notification card |  | ✓ | — the press did it | — the press opened :messages | — the first press opened :messages | — news isn't scrolled | — nothing on hover |
| A picker | a row | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | ✓ | ✓ | — nothing on hover |
| A picker | its query | ✓ | — the press did it | ✓ | as one press | ✓ | — nothing on hover |
| A picker | its frame's other rows | — no row there | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✓ | — nothing on hover |
| A picker | outside it | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | — the first press closed it | ✓ | — nothing on hover |
| A picker's preview |  | says: a press on a row of the list picks it | — the press did it | ✗ S: the preview's text selects | as one press | ✓ | — nothing on hover |
| The prompt | a completion | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✗ O: its completions scroll | — nothing on hover |
| The prompt | its input | ✓ | — the press did it | ✓ | as one press | ✗ O: its completions scroll | — nothing on hover |
| The prompt | outside it | ✓ | — the press did it | ✗ S: a drag follows the pointer's row | as one press | ✗ O: its completions scroll | — nothing on hover |
| Hover text, docs |  | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | — only the wheel is its; the rest goes under | ✓ | — only the wheel is its; the rest goes under |
| Which-key |  | ✗ O: its rows press as their keys | — goes to what's under it | — goes to what's under it | — goes to what's under it | ✗ O: its wheel | — goes to what's under it |
| The completion menu |  | ✗ O: its rows accept | — goes to what's under it | — goes to what's under it | — goes to what's under it | ✗ O: its wheel | — goes to what's under it |
| The note card, the thought card |  | ✗ O: a press is its own | — goes to what's under it | — goes to what's under it | — goes to what's under it | ✗ O: its wheel | — goes to what's under it |
| The status line | a word (`▲N waiting`, the chip, what Claude does now) | ✓ | — the press did it | ✗ W: a drag there does nothing | as one press | — nothing to scroll | ✗ W: a word's key on hover |
| The status line | the rest | ✗ W: its state words do their keys | — the press did it | ✗ W: a drag there does nothing | as one press | — nothing to scroll | — nothing on hover |
| The start screen |  | ✗ O: its recent files and keys press | — the press did it | ✗ O: nothing to select | as one press | — nothing to scroll | — nothing on hover |
| Pointing's labels |  | ✓ | — the press did it | ✗ O: pointing takes a press only | as one press | — nothing to scroll | — the press did it |

States in which a press where the keys already are does nothing and says nothing, on any surface:

| State | Press |
|---|---|
| `C-w m` moving a floating pane | ✗ O: a press on it says enter or esc ends the move |
| `C-w` under way | ✗ O: a press ends the sequence, as which-key's rows press as their keys |
