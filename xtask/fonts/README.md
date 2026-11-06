`JetBrainsMono-Regular.ttf` is JetBrains Mono 2.304 (its name table's version), copied unchanged from its release,
under the SIL Open Font License 1.1 — `OFL.txt`, from the JetBrains/JetBrainsMono repository at tag `v2.304`.

The shots' renderer (`xtask/src/shot.rs`, `find_font`) looks here last, after the system's font folders: a machine
without JetBrains Mono installed — CI's runners, `cargo xtask linux`'s image — still draws, so xtask's tests that draw
(`video::tests`, `shot::tests`) check there too. Bold and italic aren't here: they fall back to Regular, the same
cells (the font is monospaced).
