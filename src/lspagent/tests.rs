//! The java pack's Lombok agent (#228) found in a stand-in home — its Gradle and Maven caches made in a scratch folder,
//! never the real ones — for stand-in projects; nothing runs.

use std::path::{Path, PathBuf};

use super::*;

/// The java pack's agents.
fn jdtls() -> &'static [Agent] {
    let spec = crate::lsp::ServerSpec {
        name: "jdtls".into(),
        command: "jdtls".into(),
        args: vec![],
        except: vec![],
    };
    &crate::lsp::traits(&spec).agents
}

/// A scratch folder with a home (`home/`) and a project (`app/`) in it.
struct Place {
    _s: crate::scratch::Scratch,
    home: PathBuf,
    app: PathBuf,
}

fn place(name: &str) -> Place {
    let s = crate::scratch::Scratch::new(name);
    let dir = std::fs::canonicalize(s.path()).unwrap();
    let (home, app) = (dir.join("home"), dir.join("app"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&app).unwrap();
    Place { _s: s, home, app }
}

impl Place {
    fn write(&self, rel: &str, text: &str) {
        let at = self.app.join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }

    /// Lombok `v` in the home's Gradle cache (its hash folder, and its sources jar beside).
    fn gradle(&self, v: &str) -> PathBuf {
        let dir =
            self.home.join(format!(".gradle/caches/modules-2/files-2.1/org.projectlombok/lombok/{v}/ab12"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("lombok-{v}-sources.jar")), "").unwrap();
        let jar = dir.join(format!("lombok-{v}.jar"));
        std::fs::write(&jar, "").unwrap();
        jar
    }

    /// Lombok `v` in the home's Maven repository.
    fn maven(&self, v: &str) -> PathBuf {
        let dir = self.home.join(format!(".m2/repository/org/projectlombok/lombok/{v}"));
        std::fs::create_dir_all(&dir).unwrap();
        let jar = dir.join(format!("lombok-{v}.jar"));
        std::fs::write(&jar, "").unwrap();
        jar
    }

    /// What jdtls starts with besides `args` — the home's variables none, runs writing in `writes`, as `me`.
    fn found_as(&self, args: &[&str], writes: Option<&[PathBuf]>, me: crate::project::Me) -> Vec<String> {
        self.look(args, writes, me).args()
    }

    fn look(&self, args: &[&str], writes: Option<&[PathBuf]>, me: crate::project::Me) -> Found {
        let none = |_: &str| None;
        let os = crate::sandbox::Os::HOST;
        let vars = crate::sandboxpack::Vars::new(crate::sandboxpack::get(), &self.home, &none, os);
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        find(jdtls(), &args, &self.app, &|p| vars.place(p), writes, me)
    }

    fn found(&self) -> Vec<String> {
        self.found_as(&[], None, crate::project::Me::now())
    }
}

fn arg(jar: &Path) -> Vec<String> {
    vec![format!("--jvm-arg=-javaagent:{}", jar.display())]
}

const GRADLE: &str = "plugins { id 'java' }\ndependencies {\n    compileOnly 'org.projectlombok:lombok:1.18.30'\n    \
                      annotationProcessor 'org.projectlombok:lombok:1.18.30'\n}\n";

/// A Gradle build naming Lombok 1.18.30: that one's jar from Gradle's cache, though a newer one is there — not its
/// sources jar, not a snapshot. The version it names not downloaded: the newest that is.
#[test]
fn a_gradle_build_gets_the_lombok_it_names() {
    let p = place("agent-gradle");
    p.write("build.gradle", GRADLE);
    let named = p.gradle("1.18.30");
    let newest = p.gradle("1.18.38");
    p.gradle("edge-SNAPSHOT");
    assert_eq!(p.found(), arg(&named));
    std::fs::remove_file(&named).unwrap();
    assert_eq!(p.found(), arg(&newest), "its version isn't there: the newest");
}

/// A Maven module's pom naming it by its `lombok.version` property: Maven's repository's. A build that names no
/// version (Spring Boot's BOM picks it) gets the newest release in either cache at least 1.18.4 — 1.18.40 over 1.18.4,
/// numbers, not text; one older than 1.18.4 never.
#[test]
fn a_maven_build_gets_its_own_and_one_naming_none_the_newest() {
    let p = place("agent-maven");
    p.write("pom.xml", "<project><modules><module>core</module></modules></project>\n");
    p.write(
        "core/pom.xml",
        "<project>\n  <properties><lombok.version>1.18.32</lombok.version></properties>\n  <dependency>\n    \
         <groupId>org.projectlombok</groupId>\n    <artifactId>lombok</artifactId>\n    \
         <version>${lombok.version}</version>\n  </dependency>\n</project>\n",
    );
    let own = p.maven("1.18.32");
    let newest = p.maven("1.18.40");
    p.gradle("1.18.4");
    assert_eq!(p.found(), arg(&own));
    p.write(
        "core/pom.xml",
        "<dependency><groupId>org.projectlombok</groupId><artifactId>lombok</artifactId></dependency>",
    );
    assert_eq!(p.found(), arg(&newest), "no version named");
    std::fs::remove_file(&newest).unwrap();
    std::fs::remove_file(&own).unwrap();
    p.maven("1.16.22");
    assert_eq!(p.found(), arg(&p.gradle("1.18.4")), "1.16.22 is below the floor");
}

/// A version catalog names it for a build (`libs.lombok`): its `[versions]` entry.
#[test]
fn a_version_catalog_names_it() {
    let p = place("agent-catalog");
    p.write("build.gradle.kts", "dependencies { compileOnly(libs.lombok) }\n");
    p.write(
        "gradle/libs.versions.toml",
        "[versions]\nlombok = \"1.18.34\"\n[libraries]\nlombok = { module = \"org.projectlombok:lombok\", version.ref = \"lombok\" }\n",
    );
    let named = p.gradle("1.18.34");
    p.gradle("1.18.36");
    assert_eq!(p.found(), arg(&named));
}

/// Nothing changes where the project doesn't use Lombok (another project's jar in the cache), where its build
/// never downloaded one, or where your own arguments load one.
#[test]
fn nothing_where_it_isnt_used_or_found_or_yours() {
    let p = place("agent-none");
    p.write("build.gradle", "dependencies { implementation 'com.google.guava:guava:33.0.0-jre' }\n");
    p.gradle("1.18.30");
    let me = crate::project::Me::now();
    assert_eq!(p.look(&[], None, me), Found::default(), "not used: nothing missing either");
    let q = place("agent-none-cached");
    q.write("build.gradle", GRADLE);
    let found = q.look(&[], None, me);
    assert!(found.loaded.is_empty(), "not in a cache");
    assert_eq!(found.missing, ["Lombok"], "used, its jar not there yet: looked for again after the import");
    p.write("build.gradle", GRADLE);
    let mine = ["--jvm-arg=-javaagent:/opt/lombok.jar"];
    assert_eq!(p.look(&mine, None, me), Found::default(), "yours stays alone, nothing missing");
}

/// A jar is taken only where it's yours and no sandboxed run writes: the cache under a run's write place, or owned by
/// someone else (another uid: the stand-in, never a chown), passes it over for one that is.
#[test]
fn a_jar_a_run_could_have_put_there_is_never_loaded() {
    let p = place("agent-writes");
    p.write("build.gradle", GRADLE);
    let gradle = p.gradle("1.18.30");
    let maven = p.maven("1.18.20");
    let me = crate::project::Me::now();
    assert_eq!(p.found_as(&[], None, me), arg(&gradle));
    let writes = [p.home.join(".gradle")];
    assert_eq!(p.found_as(&[], Some(&writes), me), arg(&maven), "Gradle's cache is a run's: Maven's");
    let writes = [p.home.clone()];
    assert!(p.found_as(&[], Some(&writes), me).is_empty(), "both");
    let other = crate::project::Me { uid: me.uid.wrapping_add(1) };
    assert!(p.found_as(&[], None, other).is_empty(), "someone else's");
}

#[test]
fn versions_are_dotted_numbers() {
    assert_eq!(numbers("1.18.30"), Some(vec![1, 18, 30]));
    assert!(numbers("1.18.40") > numbers("1.18.4"));
    assert_eq!(numbers("1.18.31-SNAPSHOT"), None);
    assert_eq!(numbers("edge-SNAPSHOT"), None);
}
