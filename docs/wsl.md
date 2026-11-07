# tarae on WSL2

What tarae's sandbox gives Claude's commands on WSL2 depends on one setting of WSL's: its networking mode
(`.wslconfig`'s `networkingMode`). The tiers are [the Linux ones](claude-integration.md#on-linux); this page says
which one WSL2 reaches and why. Read from WSL's own source (microsoft/WSL at 19cd15ea, 2026-11-06).

## By networking mode

| Mode | Tier | Why |
|---|---|---|
| NAT (WSL's default), bridged | own where user namespaces work, else host | nothing of WSL's sits above tarae: it serves a command's sockets as on any Linux |
| mirrored, Consomme | unserved, with the session's namespaces, bridged | WSL's port tracker puts a seccomp listener on every process |

**The port tracker.** In mirrored and Consomme networking the VM's first process (mini_init) installs a seccomp
filter with a listener on itself before any distro starts, so every process inherits it — tarae too. It watches
`bind`, `listen` and `ioctl(SIOCSIFFLAGS)` to forward ports and interface changes to Windows. The kernel lets a
process have one listener in its chain, so tarae can't add its own, and serves none of a command's calls there. No
WSL setting removes it in those modes: `localhostForwarding = false` changes nothing, and `wsl.conf` comes too late.

**NAT can become Consomme.** When WSL can't create NAT's network it falls back to Consomme with a warning, and an
administrator's policy can force the mode. tarae judges by what it finds: the sandbox's panel (`space m s`, `:sandbox-blocked`) says
the tier, and Claude's report and `tarae state` the whole reason — on WSL2, that the listener is the port tracker's.

## Unserved, bridged: what a command keeps apart and shares

Every namespace works under the port tracker, so a session's keeper is born in the session's own (user, network, IPC,
PID, mount). A command keeps apart from other sessions' commands what it keeps in tier own: files (but the temp
folders), processes, signals, System V IPC, `/dev/shm`, terminals and its own loopback. Files named like keys outside
the project are hidden by mounts in the command's mount namespace, as in tier own.

**The network beyond, only through tarae's proxy (the bridge).** As the session's keeper is born it listens at the
proxy's address (`192.0.2.1:3128`) inside the session's network namespace, and hands that listener to tarae's proxy;
nothing else leaves the namespace. Each command's proxy variables carry a token of its own, as on macOS, and the proxy
tells the command by it within that session alone: each host on its card, every rule as in the served tiers. A
direct connect (raw TCP, a tool that ignores the variables) is the kernel's `Network is unreachable`, which names no
address; tarae reads it from a failed command's output and says the proxy is the only way. Within a session a command
that reads another's environment could borrow its token — the threat model's G31.

**Never offered to Windows.** The port tracker lets a `bind` or `listen` from another network namespace through
untouched, and NAT's localhost relay reads only its own namespace's sockets. The session's port, its `lo` and any
listener a command makes stay inside the session. tarae brings the session's `lo` up by netlink, never by
`SIOCSIFFLAGS`, which the tracker would pass on to Windows from any namespace.

**Not reached there**, as nothing serves the calls that would decide them:

- this machine's ports (`llm.sandbox-ports`, `llm.sandbox-localhost`, its own addresses) — an `also` for a port is
  answered with the tier and why, never a card; only `outside_sandbox` reaches one;
- unix sockets, the project's and a granted one (`unix_socket`) alike;
- a file's owner and extended attributes (`cp -a`, `rsync -a`, `install -m` fail); a file's mode and times change
  outside the write places but on the protected names, which read-only mounts seal (G29).

## A company's CA

Behind a company's TLS-inspecting proxy every certificate is re-signed with the company's own root, and tarae's proxy
relays HTTPS unread, so a command's own TLS client must trust that root. Each command gets a trust bundle the CA
variables point at ([the sandbox](claude-integration.md), #419), the same in every tier and mode, bridged too. Its
system roots are the distro's store (and the files tarae's own `SSL_CERT_FILE`/`SSL_CERT_DIR` name) — what tarae's own
`read` of a page trusts. A company root installed only in Windows' certificate store isn't in the distro's: put its
PEM file in the distro and name it in your config:

```toml
[llm]
sandbox-ca = ["~/corp-root.pem"]
```

`llm.sandbox-ca` is yours only (a project can't set it). Reading Windows' store from WSL is a later piece.

## Still to come

- **Windows programs and `/mnt/c`**: a test pinning that `cmd.exe`, `powershell.exe` and `clip.exe` fail in a
  command, and a check of what Claude's `read` and a command reach under the Windows home (`/mnt/c/Users/…`, with its
  `.ssh`, `.aws`, `.kube`) — counted as a home once checked.
- **The root mounted twice** (#420): WSL mounts the distro's root again at `/mnt/wslg/distro`, where a key hidden at
  its own path reads; to be hidden whole in the command's mount namespace.
- **G29 by read-only mounts**: every file outside the write places read-only in the command's mount namespace, so the
  kernel refuses mode and times there — planned apart.
