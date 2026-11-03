# A host and its interface

tarae can run with no terminal of its own — a **host** — and be shown in another terminal by **`tarae attach`**. The
host keeps everything: your buffers, the sessions and their Claudes, runs, tests, cards. The client only shows what the
host draws and sends your keys and mouse back. Close the client and the host works on; attach again and you're where
you were, a turn that ended meanwhile on screen.

This is opt-in, and on this machine only for now: the client reaches the host by a unix socket. `tarae` alone runs
as it always has, in the terminal it starts in.

## Running a host and attaching

```sh
tarae --host [FILE]...      # in a zellij pane, or with & — it ignores SIGHUP, quits on SIGTERM or SIGINT
tarae attach [--pid PID]    # in any terminal: the host's screen, your keys its
```

Without `--pid`, `attach` picks the host `tarae state` would: the one running, or the one whose session holds the
folder you're in (several and none does: they're listed). The client says what it is as it connects — its size, a
light or dark ground, whether it draws curly underlines, whether it's inside zellij — and the host draws for that from
the next frame. The first frame goes whole; after it, only what changed.

- **`:detach`** lets the client go and leaves the host working. The client restores its terminal and says
  `detached — tarae attach comes back`. In a plain `tarae` it says it needs `--host`.
- **A second `tarae attach` takes over**: one interface at a time. The first says `taken over by another tarae attach`
  and exits.
- **Closing or killing the client** is a detach: the host notices the socket close and draws nothing until the next
  attach.
- **The host ending.** A quit (`:q`) closes the client too, which then prints what the quit didn't write. A host that
  ended unsaid (a crash) leaves its last frame up with the client's own line, `the host ended (…) — r attach again · q
  quit`. The next `tarae --host` offers the crash's recovery, as `tarae` does.
- **`:restart`** restarts the host into the new build, still `--host`, and the client execs into the same binary — your
  terminal stays as it was and the new host's first frame comes in it.

While nothing is attached the host draws nothing and wakes for nothing on screen: no frames, no ticks, no idle
`/compact` (there's nobody reading). Its work goes on — a turn ends, a run finishes, a card waits — and is in the first
frame of the next attach. `tarae state --pid <host>` answers as ever; its `interface` is `null` while detached, `{"kind":
"wire", "presented": N}` while attached.

## What's refused, and why

The socket is `$XDG_STATE_HOME/tarae/ui/<pid>.attach`, made as the state socket is: a folder only you can enter, a
socket only you can open, bound before the host starts any thread. Past that, an attach is refused — with its reason on
the client's terminal — when:

- **it's another user's** (the socket's peer is checked, not trusted);
- **it's another build** — the client and the host must be the same tarae (protocol and version, both named in the
  refusal: attach with the host's own build);
- **its first message isn't `Hello` within 5 s**, or its bytes don't read: the connection goes;
- **it runs under one of Claude's processes** — Claude has its own view of the screen (`tarae state`'s, its `ui` tool),
  never your keys;
- **it runs inside the host's own terminal panel** — it would draw the host inside itself; attach from another terminal.

## What's the interface's

What you touch is the machine the client runs on, not the host's:

- **The clipboard.** A copy to the clipboard is asked of the client, which copies with its own
  programs, else its terminal's OSC 52; a paste reads its clipboard. Said as it's done, from the client's answer. With
  nothing attached a copy or paste says `no interface is attached (tarae attach)`. An answer that doesn't come within
  10 s, or a client that goes first, is said — never carried over to the next client.
- **Sound** plays on the client's speaker (`editor.sound`; macOS clients only, yet). A sound held up past half a second
  is dropped rather than played late.
- **`:web-open`** opens the address in the client's browser.
- **Times shown** — ages, `warm until`, a limit's reset, a card's times — are in the client's time zone; the host's
  logs and files keep the host's.
- **The screen it shows.** The client holds its own copy of the screen and redraws it whole when things rest, so the
  wire never carries a redraw. It acks each frame once it's written to its terminal.

## "Drawn" means shown to you

Every rule tarae has that waits for something to be on screen — a weighty card answers only once you've seen it, a cut
card's `y` only once its text was read whole, a short high-risk change under the cursor (`space v h`) only once its lines
were on screen — counts a frame only once the client presented it, never as it's drawn. Your keys and presses each say
the last frame the client presented, and are judged by it:

- a card's key typed before the frame that drew it was presented is refused, said on the card
  (`y came before this card was in sight — not taken`);
- **a press on an older frame** acts only if the same thing is under it now — the same surface and part, the same
  document in a code pane, the same card at the same `rev`, the same text on a list's row. Else it's refused (`the screen
  moved under that press — not taken`) and its drag goes nowhere. The wheel and hover go by the screen now: a misplaced
  scroll moves nothing of yours;
- **`C-s`** sends Claude the frame the client presented, not one drawn after it;
- **at an attach, "in sight" starts over**: what the client before was shown doesn't count for this one until a frame of
  its own shows it.

The terminal tarae runs in presents a frame as it's written, so locally all of this is what it always was — but one
case: `space v h` in the same instant as the key that brought a short high-risk change under the cursor, before any frame
showed it, is refused and said on every interface.

## A slow client

The host never waits on the client. A frame drawn goes whole into a mailbox; a thread of the host's finds what changed
against the last frame it sent and writes it, with at most two frames sent and not acked. A frame not sent yet when a
newer one comes is replaced: a slow client skips frames, never shows half of one, and comes to the newest once things
rest. A client that takes nothing for 30 s, or leaves 16 MB unread, is taken for gone.

Measured on this Mac (`cargo perf`, release): an attach to its first frame 8.8 ms; a key in a 200k-line file to its
frame on the client's terminal 0.27–0.36 ms (p90 0.33–0.44), against 0.25–0.32 ms in the terminal tarae runs in, ~340
bytes a key. A client whose terminal reads 12 KB/s, sent 40 half-page scrolls at once: the host's own answers (`tarae
state`) 7.2 ms against 7.3 ms quiet, the client showing 5 frames for the 40 and the newest after 8 s, as fast as its
terminal took them.
