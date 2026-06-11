//! End-to-end tests that run the compiled fews-precommit binary the same way
//! pre-commit does: from the repository root, with staged file paths as
//! arguments.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

const EXPLORER_DEBUG: &str = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
<explorer xmlns=\"http://www.wldelft.nl/fews\">\r\n\
\t<!-- set to false only for local debugging -->\r\n\
\t<adjustSystemTimeAutomatically>false</adjustSystemTimeAutomatically>\r\n\
</explorer>\r\n";

/// Temporary directory that mimics a FEWS configuration repository root.
struct Repo {
    root: PathBuf,
}

impl Repo {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "fews-precommit-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        Repo { root }
    }

    fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.root.join(rel)).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fews-precommit"))
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn fix_mode_reverts_and_fails_then_passes_on_second_run() {
    let repo = Repo::new();
    repo.write("SystemConfigFiles/Explorer.xml", EXPLORER_DEBUG);

    // First run: the debug value is reverted and the hook fails the commit.
    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("is 'false', must be 'true'"));
    let fixed = repo.read("SystemConfigFiles/Explorer.xml");
    assert_eq!(fixed, EXPLORER_DEBUG.replace(">false<", ">true<"));

    // Second run: nothing left to fix, the commit may proceed.
    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "");
}

#[test]
fn fix_preserves_declaration_comments_and_crlf_exactly() {
    let repo = Repo::new();
    repo.write("SystemConfigFiles/Explorer.xml", EXPLORER_DEBUG);
    repo.run(&["SystemConfigFiles/Explorer.xml"]);

    // The only difference must be the enforced value itself.
    let fixed = repo.read("SystemConfigFiles/Explorer.xml");
    assert_eq!(fixed, EXPLORER_DEBUG.replace(">false<", ">true<"));
    assert!(fixed.contains("\r\n"));
    assert!(fixed.contains("<!-- set to false only for local debugging -->"));
}

#[test]
fn check_mode_reports_without_modifying() {
    let repo = Repo::new();
    repo.write("SystemConfigFiles/Explorer.xml", EXPLORER_DEBUG);

    let output = repo.run(&["--check", "SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("must be 'true'"));
    assert!(!stdout(&output).contains("reverted"));
    assert_eq!(repo.read("SystemConfigFiles/Explorer.xml"), EXPLORER_DEBUG);
}

#[test]
fn files_not_matching_any_rule_are_ignored() {
    let repo = Repo::new();
    let other = repo.write(
        "Config/MapLayerFiles/Rivers.xml",
        "<adjustSystemTimeAutomatically>false</adjustSystemTimeAutomatically>",
    );

    let output = repo.run(&["Config/MapLayerFiles/Rivers.xml"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(fs::read_to_string(other).unwrap().contains(">false<"));
}

#[test]
fn rule_matches_explorer_xml_nested_deeper_in_the_tree() {
    let repo = Repo::new();
    repo.write(
        "Trunk/Config/SystemConfigFiles/Explorer.xml",
        EXPLORER_DEBUG,
    );

    let output = repo.run(&["Trunk/Config/SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(repo
        .read("Trunk/Config/SystemConfigFiles/Explorer.xml")
        .contains(">true<"));
}

#[test]
fn config_file_in_repo_root_is_discovered_automatically() {
    let repo = Repo::new();
    repo.write(
        ".fews-precommit.toml",
        r#"
[[rules]]
path = "**/*.xml"
tag = "showDebugMenu"
expected = "false"
"#,
    );
    repo.write(
        "Settings.xml",
        "<root><showDebugMenu>true</showDebugMenu></root>",
    );

    let output = repo.run(&["Settings.xml"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        repo.read("Settings.xml"),
        "<root><showDebugMenu>false</showDebugMenu></root>"
    );
}

#[test]
fn explicit_config_overrides_default_discovery() {
    let repo = Repo::new();
    repo.write(
        "tools/rules.toml",
        r#"
[[rules]]
path = "**/*.xml"
tag = "logLevel"
expected = "INFO"
forbidden = ["DEBUG", "TRACE"]
"#,
    );
    repo.write("Log.xml", "<root><logLevel>DEBUG</logLevel></root>");
    repo.write("Custom.xml", "<root><logLevel>WARN</logLevel></root>");

    let output = repo.run(&["--config", "tools/rules.toml", "Log.xml", "Custom.xml"]);
    assert_eq!(output.status.code(), Some(1));
    // DEBUG is in the forbidden list and gets reverted...
    assert_eq!(
        repo.read("Log.xml"),
        "<root><logLevel>INFO</logLevel></root>"
    );
    // ...but WARN is not forbidden and stays untouched.
    assert_eq!(
        repo.read("Custom.xml"),
        "<root><logLevel>WARN</logLevel></root>"
    );
}

#[test]
fn utf8_bom_is_preserved() {
    let repo = Repo::new();
    let mut content = Vec::from(&b"\xef\xbb\xbf"[..]);
    content.extend_from_slice(EXPLORER_DEBUG.as_bytes());
    let path = repo.write("SystemConfigFiles/Explorer.xml", &content);

    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(1));
    let fixed = fs::read(path).unwrap();
    assert!(fixed.starts_with(b"\xef\xbb\xbf"));
    assert!(String::from_utf8(fixed).unwrap().contains(">true<"));
}

#[test]
fn invalid_xml_is_an_error() {
    let repo = Repo::new();
    repo.write("SystemConfigFiles/Explorer.xml", "<explorer><unclosed>");

    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("error"));
}

#[test]
fn invalid_config_is_an_error() {
    let repo = Repo::new();
    repo.write(".fews-precommit.toml", "[[rules]]\nnot_a_key = 1\n");
    repo.write("SystemConfigFiles/Explorer.xml", EXPLORER_DEBUG);

    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_option_is_an_error() {
    let repo = Repo::new();
    let output = repo.run(&["--bogus"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn well_formed_flag_catches_syntax_errors_in_unmatched_files() {
    let repo = Repo::new();
    repo.write("RegionConfigFiles/Broken.xml", "<a><oops></a>");

    // Without the flag the file matches no rule and is skipped entirely.
    let output = repo.run(&["RegionConfigFiles/Broken.xml"]);
    assert_eq!(output.status.code(), Some(0));

    // With --well-formed every staged XML file must at least parse.
    let output = repo.run(&["--well-formed", "RegionConfigFiles/Broken.xml"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn remove_attribute_and_required_rules_work_end_to_end() {
    let repo = Repo::new();
    repo.write(
        ".fews-precommit.toml",
        r#"
[[rules]]
path = "**/*.xml"
tag = "debugSettings"
remove = true

[[rules]]
path = "**/*.xml"
tag = "export"
attribute = "debug"
expected = "false"

[[rules]]
path = "**/*.xml"
tag = "general/cardinalTimeStep"
expected = "1"
required = true
"#,
    );
    repo.write(
        "Workflow.xml",
        "<workflow>\n\t<debugSettings><verbose/></debugSettings>\n\t<export debug=\"true\"/>\n</workflow>",
    );

    let output = repo.run(&["Workflow.xml"]);
    assert_eq!(output.status.code(), Some(1));
    let out = stdout(&output);
    assert!(out.contains("<debugSettings> is not allowed -- removed"));
    assert!(out.contains("must be \"false\""));
    assert!(out.contains("required <general/cardinalTimeStep> is missing"));
    assert_eq!(
        repo.read("Workflow.xml"),
        "<workflow>\n\t<export debug=\"false\"/>\n</workflow>"
    );
}

#[test]
fn no_files_is_a_no_op() {
    let repo = Repo::new();
    let output = repo.run(&[]);
    assert_eq!(output.status.code(), Some(0));
}

/// The repository's own example files must demonstrate a working setup.
#[test]
fn shipped_example_is_fixed_by_shipped_example_config() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = Repo::new();
    repo.write(
        "SystemConfigFiles/Explorer.xml",
        fs::read(manifest.join("example/SystemConfigFiles/Explorer.xml")).unwrap(),
    );
    repo.write(
        ".fews-precommit.toml",
        fs::read(manifest.join("example/.fews-precommit.toml")).unwrap(),
    );

    let output = repo.run(&["SystemConfigFiles/Explorer.xml"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(repo
        .read("SystemConfigFiles/Explorer.xml")
        .contains("<adjustSystemTimeAutomatically>true</adjustSystemTimeAutomatically>"));
}
