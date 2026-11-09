# Security

## Reporting a vulnerability

Report it privately — not as a public issue — through GitHub's private reporting (the repository's **Security** tab
→ **Report a vulnerability**, https://github.com/tarae-sh/tarae/security/advisories/new) or by email to
**security@sinabro.io**. Please include:

- what you did (the steps, a script or a repository that shows it),
- what happened, and what you expected instead,
- the OS and version (on Linux the kernel too, and the tier a run's report names),
- tarae's commit (`git rev-parse HEAD` in your checkout).

## What's in scope

- tarae's sandbox for Claude's commands: Seatbelt on macOS; Landlock, seccomp and the keeper on Linux.
- The gates on Claude's own tools: reads outside the project asked on a card, edits reviewed, no shell — the
  `claude` process itself runs as you, as in every other tool.
- Credential masking: a masked variable reaches a run as a stand-in, and the proxy puts the real value in only
  toward its hosts.
- The trust gates: a project's settings (`.tarae.toml`), a repository's git config and hooks, a repository's
  Claude settings.
- The egress proxy Claude's commands reach the network through.
- The plugins' WebAssembly runtime and the permissions a plugin is given.
- The UI socket (`tarae state`): only this user's processes reach it, and claude's are refused.

[The threat model](docs/threat-model.md) says what tarae claims to keep from a run and what it doesn't. Its known
gaps ([§4](docs/threat-model.md#4-gaps-in-one-list)) aren't new reports — unless you find one is worse than it
says.

## Supported versions

`main` only. There are no releases yet.

## How reports are handled

An acknowledgement within 7 days, a fix on `main`, and credit in the commit and the release notes if you want it.
There's no bounty.
