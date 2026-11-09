//! The plugin examples (`plugin-sdk/examples/`, PD1's H2), in the binary: `tarae plugin example <name>` prints one, and
//! `tarae plugin new <dir> --from <name>` (`:plugin-new`, the `new` of Claude's `plugin` tool) starts a crate from one.
//! So Claude reads them where it works — the user's project —, never only in tarae's repository.

/// An example: its name (its folder), its idea in a few words, its `src/lib.rs`.
pub struct Example {
    pub name: &'static str,
    pub idea: &'static str,
    pub lib: &'static str,
}

macro_rules! example {
    ($name:literal, $idea:literal) => {
        Example {
            name: $name,
            idea: $idea,
            lib: include_str!(concat!("../../plugin-sdk/examples/", $name, "/src/lib.rs")),
        }
    };
}

/// Every example, in the order the README lists them.
pub const EXAMPLES: &[Example] = &[
    example!("sort-lines", "a command over the selections"),
    example!("yaml-tabs", "an event with a language filter"),
    example!("todo-hints", "hints tied to a buffer version"),
    example!("word-tally", "a panel with its own keys"),
    example!("md-outline", "a side tab"),
    example!("read-notes", "reading a file under a `read` pattern"),
    example!("git-branch", "a program run with `run(prog)`"),
    example!("http-status", "an HTTP call through the SDK"),
    example!("final-newline", "a before-save check"),
    example!("stamps", "state kept with `write-own`"),
];

/// Example `name` — else why, with the names there are.
pub fn find(name: &str) -> Result<&'static Example, String> {
    EXAMPLES.iter().find(|e| e.name == name).ok_or_else(|| {
        let names: Vec<&str> = EXAMPLES.iter().map(|e| e.name).collect();
        format!("no example {name} — there are {}", names.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every folder of `plugin-sdk/examples` is one of `EXAMPLES`, a member of its workspace and a line of its README —
    /// and the other way round: an example added is in the binary, the index and the build, or this fails.
    #[test]
    fn every_example_is_listed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin-sdk/examples");
        let mut folders: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().join("Cargo.toml").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        folders.sort();
        let mut listed: Vec<String> = EXAMPLES.iter().map(|e| e.name.to_string()).collect();
        listed.sort();
        assert_eq!(folders, listed, "the folders and EXAMPLES");
        let manifest: toml::Table =
            toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
        let members: Vec<&str> =
            manifest["workspace"]["members"].as_array().unwrap().iter().filter_map(|m| m.as_str()).collect();
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        for e in EXAMPLES {
            assert!(members.contains(&e.name), "{} in the workspace's members", e.name);
            let line = format!("- [`{0}`]({0}/src/lib.rs) — {1}", e.name, e.idea);
            assert!(readme.contains(&line), "README: {line}");
            // (one idea, said first: its doc comment opens with it)
            assert!(e.lib.starts_with("//! "), "{} opens with what it shows", e.name);
        }
        assert_eq!(members.len(), EXAMPLES.len());
    }

    #[test]
    fn an_unknown_example_names_the_ones_there_are() {
        let e = find("nope").err().unwrap();
        assert!(e.contains("no example nope") && e.contains("sort-lines") && e.contains("stamps"), "{e}");
        assert_eq!(find("stamps").unwrap().name, "stamps");
    }
}
