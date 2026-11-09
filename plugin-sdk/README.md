# tarae plugin SDK

- [`wit/tarae.wit`](wit/tarae.wit): the plugin API — what a plugin sees and what it can answer
- [`template/`](template): what `tarae plugin new` writes — with a test through the mock host, a
  `.cargo/config.toml` for `wasm32-unknown-unknown`, and the SDK, the rules and the WIT vendored in its `tarae/`
- [`demo/`](demo): a plugin with a command for every action, and some that misbehave on purpose. Its build,
  [`demo.wasm`](demo.wasm), is what tarae's tests load — a fixture with bindings of its own ([its README](demo/README.md))
- [`sdk/`](sdk): `tarae-plugin`, the SDK — the WIT's bindings (it owns them: `Guest`, `export!`, the types), the time
  and randomness tarae hands over, HTTP read as it comes, TLS over a plugin's connections, and the mock host a
  plugin's tests drive it through (feature `test`)
- [`rules/`](rules): `tarae-plugin-rules`, the rules tarae holds a plugin to and the words it says them in — tarae
  and the mock host judge by the same code; with them the list of file names that look like keys
- [`kubernetes/`](kubernetes): the cluster in the code — each object's state beside its manifest, a side tab, in pure Rust (kube-core, k8s-openapi, rustls)
- [`db/`](db): the database in the code — PostgreSQL, MySQL and Redis
- [`keycast/`](keycast): the keys you press, as keycaps in the asks' corner — for a recording; it hears `keys` alone

How to write and install a plugin: [docs/plugins.md](../docs/plugins.md).

## License

What a plugin is built from — [`wit/`](wit), [`template/`](template), [`sdk/`](sdk) and [`rules/`](rules), its
[list of names like keys](rules/src/secret-names.txt) too — is under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your choice, so a plugin can be under any license, closed ones
too. tarae's own plugins here — [`demo/`](demo), [`db/`](db), [`kubernetes/`](kubernetes), [`keycast/`](keycast) — are under the same
two; the servers' bytes in `db`'s test transcripts are their projects' ([NOTICE](db/NOTICE)). tarae itself stays
GPL-3.0-or-later.
