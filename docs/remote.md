# A host and its interface

tarae can run with no terminal of its own — a **host** — and be shown in another terminal by **`tarae attach`**. The
host keeps everything: your buffers, the sessions and their Claudes, runs, tests, cards. The client only shows what the
host draws and sends your keys and mouse back. Close the client and the host works on; attach again and you're where
you were, a turn that ended meanwhile on screen.

This is opt-in. The client reaches a host on this machine by a unix socket, and one on another machine through ssh
([Over SSH](#over-ssh)). `tarae` alone runs as it always has, in the terminal it starts in.

## Running a host and attaching

```sh
tarae --host [FILE]...      # in a zellij pane, or with & — it ignores SIGHUP, quits on SIGTERM or SIGINT
tarae attach [--pid PID]    # in any terminal: the host's screen, your keys its
```

Without `--pid`, `attach` picks the host `tarae state` would: the one running, or the one whose session holds the
folder you're in (several and none does: they're listed). The client says what it is as it connects — its size, a
light or dark ground, whether it draws curly underlines, inside zellij the keys zellij takes there (`editor.zellij-keys`
in your config on the client's machine, the one setting a client reads) — and the host draws for that from the next
frame: a hint for a key zellij takes there says its twin. The first frame goes whole; after it, only what changed.

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

## Over SSH

```sh
tarae attach ssh://[USER@]HOST[:PORT][/PATH][?pid=N] [--remote-tarae PATH] [--stats]
```

The host runs on the other machine as above (`tarae --host`, in a zellij pane there); the client runs here and reaches
it through ssh. No port is opened for it: sshd, or Tailscale SSH, is the only thing listening, and ssh is the identity.

- **The address.** `HOST` goes to ssh as you wrote it, so your `~/.ssh/config` applies: its aliases, keys, jump hosts
  and `ControlMaster`. No ssh options pass through the address; that file is the place for them. `PATH` picks the host
  whose project holds it: `/~/x` is under the remote home, `/x` is absolute. With no path, the only host running there
  is the one; several are listed. `?pid=N` names one. A password, an empty port, or a user or host starting with `-`
  is refused, said.
- **What runs.** `ssh -T -e none -o ServerAliveInterval=15 -o ServerAliveCountMax=3 [-p PORT] [-l USER] HOST -- exec
  "$SHELL" -lc 'tarae attach --stdio …'`: no pty (a pty would make OpenSSH send each key on a 20 ms clock), no escape
  character, and a login shell, so the PATH your `~/.profile` sets (Homebrew, cargo) finds tarae. `--remote-tarae PATH`
  names a tarae that isn't on it. The quoting holds in sh, bash, zsh and fish alike.
- **The relay.** `tarae attach --stdio` on the other machine says a line of its own (`TARAE-STDIO 1`, then `ok` with
  the host's pid and project, or `no` with why and the hosts it found), then copies bytes both ways and reads none of
  them. To the host it's a local attach: every check under [What's refused](#whats-refused-and-why) applies to it.
  Whatever an rc file prints before that line, up to 64 KB, is passed over.
- **ssh may ask.** The terminal stays as it was until the host has answered, so a password or a new host key is asked
  as in any ssh. ssh's own words go on the terminal until then, and after it the last of them is kept for anything said
  of the link.
- **Each failure in its own words**, the terminal restored, exit 1: ssh's (`tarae attach: ssh mac: Permission denied
  (publickey)`), no tarae there (exit 127, with `--remote-tarae` as the way), a tarae there from before step 5
  (`unknown: --stdio`: update it), no host there, a host that refuses. **No host there** says where it looked and how
  to start one. Where the ssh session is one a Mac starts outside your login (Tailscale SSH's), it adds that a host
  started from there would have no Claude sign-in: the login keychain isn't reachable from it. Start the host at the Mac,
  in a zellij pane there; attaching to it over ssh later changes nothing for its Claude.

### The version rule

The two sides must speak the same protocol, its number `V`; the builds needn't match. The build, the version and the
commit with a `+` when the tree differs from it (`tarae --version`), is only told: in a refusal, in the host's log for
each attach, in the client's log. So a laptop's build from a push days old keeps working while the Mac's tarae restarts
into new builds, until a change of what crosses the wire takes `V` up. Then the attach is refused with both protocols
and both builds, and the laptop's tarae is built again. `V` is pinned by `src/snapshots/wire.txt`: every message, every
key and mouse kind, and the escapes the client reads; a change there with the same `V` fails tarae's tests.

### The way back

**A sleeping screen never loses the session.** A link that drops (ssh exits, the network goes, the laptop sleeps) comes
back by itself:

- The last frame stays up, and nothing is said while the way back works. ssh runs again at once, asking nothing
  (`BatchMode`), to the same host. Its first frame is whole.
- Keys typed while it's down go nowhere. They're never queued to land on a frame nobody saw.
- A wake is noticed: when the wall clock jumped past the monotonic one (a suspend), or the terminal says it has the
  focus again, the client sends a `Ping`. No answer within 3 s and the link is taken for gone and tried again. Nothing
  is sent on a clock: an idle client is silent. ssh's own `ServerAlive` (15 s × 3) catches a drop nobody noticed.
- Only when a try fails, or the first takes past 3 s, does a line go over the last frame: `the link to mac dropped:
  <why> — again in 4.0s · r now · q quit`. Tries come after 1, 2, 4 … 30 s, or at once on a wake. `r` runs ssh as at the
  start, the terminal given back while it may ask for a password (a Tailscale SSH `check` rule's sign-in, too).
- Each way back is logged: `back in 61ms: the link down to a frame presented`.
- **A `Gone` never comes back by itself**: `:detach` (`detached — tarae attach ssh://mac/… comes back`), another client
  taking over (two clients would take the host from each other in a loop), `:q`, and a host that refused (another
  protocol after a restart; `r` tries again).
- **`:restart` on the host**: the client runs ssh again with the relay waiting up to 60 s for the new host's socket,
  and is attached again, the same client. Its new binary is the other machine's; nothing is exec'd here.

### Measured

`tarae attach --stats` says at its end, once the terminal is restored, what it measured by its own clock: the first
frame from its start (ssh's handshake in it); each key to the first frame the host drew after handling it (median,
p90, the frame's bytes a key); and each way back, from the link down to the new link's first frame.

- Over this Mac's loopback sshd (piece 0, OpenSSH 10.3, no pty): a key 0.68 ms median, 1.06 ms p90. With a pty, 10.5
  and 23.8 ms: OpenSSH's `ObscureKeystrokeTiming`.
- Over a stand-in ssh (the e2e tests'): a link cut to a live frame 35–66 ms; a link frozen and a focus back, 3.1 s,
  of which 3 s is the `Ping`'s wait.
- Over a real ssh into a Lima VM (`cargo xtask remote`, run in a terminal of your own; Debian 12 on `vz`, `ssh -T`, no
  shared connection; 2026-11-05): the first frame 506 ms from the client's start; a key 1.07 ms median, 1.37 ms p90
  over 90 keys, 1,652 bytes a key; a link cut back to a live frame in 86–109 ms (median 94 ms), each a whole ssh
  handshake; a link frozen and a focus back, 3.1 s, of which 3 s is the `Ping`'s wait.
- Over the tailnet from WSL: the list below.

### From Windows: WSL to this Mac over the tailnet

The client is a Linux tarae built in WSL; ssh is the distro's OpenSSH; the Mac's side is Tailscale SSH (no key on the
Mac, no Remote Login). To try it at work, in order, writing down what each says:

1. **ssh alone.** In WSL: `ssh <the Mac's tailnet name>`. Tailscale SSH needs no key. If its rule is in `check` mode, it
   prints a sign-in address: open it in Windows' browser and note that it asked. If the name doesn't resolve, run
   `tailscale ip -4 <the Mac's name>` in PowerShell and use the `100.x` address instead, noting which one worked. Then
   `exit`.
2. **The link.** In WSL: `ssh -V`. In PowerShell: `tailscale ping <the Mac's name>`, noting whether it says `via DERP(…)`
   or a direct address, and its time.
3. **A name for it.** In WSL's `~/.ssh/config`:
   ```
   Host mac
     HostName <the Mac's tailnet name or 100.x address>
   ```
4. **tarae in WSL.** `sudo apt install build-essential pkg-config git curl`, rustup if it isn't there, then from a
   clone of the pushed branch: `cargo build --release`. The binary is `target/release/tarae`; put it on your PATH.
5. **The host at the Mac.** Before leaving, at the Mac: `tarae --host` in a zellij pane in the project (it ignores
   SIGHUP and outlives the pane's terminal). Over ssh from WSL, `tarae attach ssh://mac` with no host there says so,
   and why one shouldn't be started from that session.
6. **Attach.** In Windows Terminal, in WSL: `tarae attach --stats ssh://mac/~/Workspace/tarae`. Note how long the
   first frame took.
7. **Type.** A minute of `j`/`k` and some text in insert mode, then `:detach`. Copy the three `tarae attach --stats:`
   lines.
8. **Sleep.** Attach again. Lock Windows, or close the laptop's lid, for a minute or more. Wake it. Note whether the
   last frame stayed up, whether any line was said, and whether the keys work at once. The time it took is the last
   `back in` line of `~/.local/state/tarae/attach.log` in WSL.
9. **The interface's.** `y` on a selection, then paste in a Windows program. `:web-open https://tarae.sh` should open
   Windows' browser. Inside zellij in WSL, a key hint for a `C-` key zellij takes should say its twin.
10. **Bring back** what each step said: the stats lines, `ssh -V`, `tailscale ping`'s line, step 8's three answers and
    its `back in` time.

## What's refused, and why

The socket is `$XDG_STATE_HOME/tarae/ui/<pid>.attach`, made as the state socket is: a folder only you can enter, a
socket only you can open, bound before the host starts any thread. Past that, an attach is refused — with its reason on
the client's terminal — when:

- **it's another user's** (the socket's peer is checked, not trusted);
- **it speaks another protocol** — another build is let in while its protocol is the host's (two machines' builds
  mostly differ); the refusal names both protocols and both builds;
- **its first message isn't `Hello` within 5 s**, or its bytes don't read: the connection goes;
- **it runs under one of Claude's processes** — Claude has its own view of the screen (`tarae state`'s, its `ui` tool),
  never your keys;
- **it runs inside the host's own terminal panel** — it would draw the host inside itself; attach from another terminal.
  Through ssh the processes' line ends at sshd, so the client says which panel it runs in, if any (its `TARAE_UI` and
  machine), and the host refuses its own.

The client itself refuses to run under a run of Claude's (`TARAE_RUN`, set in every one, sandboxed or not), and
Claude's `run` refuses any line that runs `tarae attach`: an interface is yours. Port 22 is one of the sandbox's risky
ports (a shell as you).

## What's the interface's

What you touch is the machine the client runs on, not the host's:

- **The clipboard.** A copy to the clipboard is asked of the client, which copies with its own
  programs, else its terminal's OSC 52; a paste reads its clipboard. Said as it's done, from the client's answer. With
  nothing attached a copy or paste says `no interface is attached (tarae attach)`. An answer that doesn't come within
  10 s, or a client that goes first, is said — never carried over to the next client. The client answers a copy, a
  paste or an address opened only within 5 s of a key, a press or a paste you sent it, else refuses and says so: a host
  never reads or sets your clipboard, nor opens your browser, unprompted.
- **Sound** plays on the client's speaker (`editor.sound`; macOS clients only, yet). A sound held up past half a second
  is dropped rather than played late.
- **`:web-open`** opens the address in the client's browser: `open` on macOS; in WSL `wslview` where it's there, else
  Windows' `explorer.exe`; `xdg-open` elsewhere.
- **`:sh`** runs on the host, and while an interface is attached without zellij's variables: a `zellij run` would act
  in the host's zellij, out of your sight.
- **The client's log** is its own, `attach.log` beside `tarae.log` in the state folder.
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
