use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use globset::{Glob, GlobMatcher};
use quick_xml::events::{BytesCData, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use serde::Deserialize;

const DEFAULT_CONFIG_NAME: &str = ".fews-precommit.toml";
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

const HELP: &str = "\
fews-precommit: reverts Delft-FEWS XML configuration values to their required
defaults, so debugging/test settings never get committed.

Usage: fews-precommit [OPTIONS] [FILES]...

Arguments:
  [FILES]...  XML files to process (pre-commit passes the staged files)

Options:
      --config <PATH>  Path to the rules file (TOML). Defaults to
                       .fews-precommit.toml in the current directory;
                       built-in defaults are used when neither is present
      --check          Only report violations, do not modify files
      --well-formed    Also parse files that no rule matches, so any XML
                       syntax error fails the hook
  -h, --help           Print help
  -V, --version        Print version
";

struct Cli {
    config: Option<PathBuf>,
    check: bool,
    well_formed: bool,
    files: Vec<PathBuf>,
}

fn parse_args() -> Result<Cli> {
    let mut cli = Cli {
        config: None,
        check: false,
        well_formed: false,
        files: Vec::new(),
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--check") => cli.check = true,
            Some("--well-formed") => cli.well_formed = true,
            Some("--config") => {
                let value = args.next().context("--config requires a path")?;
                cli.config = Some(PathBuf::from(value));
            }
            Some("--help" | "-h") => {
                print!("{HELP}");
                std::process::exit(0);
            }
            Some("--version" | "-V") => {
                println!("fews-precommit {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Some(other) if other.starts_with('-') => bail!("unknown option '{other}'"),
            _ => cli.files.push(PathBuf::from(arg)),
        }
    }
    Ok(cli)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    /// Glob matched against the file path, e.g. "SystemConfigFiles/Explorer.xml".
    path: String,
    /// Name of the XML element to enforce. May be a path like
    /// "general/startupTask" to disambiguate elements that occur in several
    /// places; it is matched against the end of the element nesting.
    tag: String,
    /// The value the element (or attribute) must have.
    #[serde(default)]
    expected: Option<String>,
    /// If non-empty, only these values are reverted; anything else is left alone.
    #[serde(default)]
    forbidden: Vec<String>,
    /// Enforce this attribute of the element instead of its text content.
    #[serde(default)]
    attribute: Option<String>,
    /// Compare values case-insensitively ("true" == "True").
    #[serde(default)]
    ignore_case: bool,
    /// The element must be present in the file; reported (not fixable) when missing.
    #[serde(default)]
    required: bool,
    /// The element must not be present at all; it is deleted on fix.
    #[serde(default)]
    remove: bool,
}

impl Rule {
    fn is_value_rule(&self) -> bool {
        !self.remove && self.attribute.is_none()
    }

    fn values_equal(&self, a: &str, b: &str) -> bool {
        if self.ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    /// A non-empty value violates the rule when it differs from `expected`
    /// and, if a forbidden list is given, appears on that list.
    fn is_violation(&self, value: &str) -> bool {
        let expected = self.expected.as_deref().unwrap_or_default();
        !self.values_equal(value, expected)
            && (self.forbidden.is_empty()
                || self.forbidden.iter().any(|f| self.values_equal(f, value)))
    }
}

struct CompiledRule {
    rule: Rule,
    matcher: GlobMatcher,
    /// Matcher for "**/<path>" so that bare relative patterns like
    /// "SystemConfigFiles/Explorer.xml" also match files deeper in the tree.
    suffix_matcher: Option<GlobMatcher>,
}

impl CompiledRule {
    fn matches(&self, path: &str) -> bool {
        self.matcher.is_match(path)
            || self
                .suffix_matcher
                .as_ref()
                .is_some_and(|m| m.is_match(path))
    }
}

#[derive(Debug)]
struct Change {
    message: String,
    /// What fix mode does about it ("reverted", "removed", "added");
    /// None when the violation cannot be fixed automatically.
    action: Option<&'static str>,
}

fn main() -> ExitCode {
    let cli = match parse_args() {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("fews-precommit: error: {err:#}");
            return ExitCode::from(2);
        }
    };
    match run(&cli) {
        Ok((false, false)) => ExitCode::SUCCESS,
        Ok((_, true)) => ExitCode::from(2),
        Ok((true, false)) => ExitCode::from(1),
        Err(err) => {
            eprintln!("fews-precommit: error: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// Returns (violations found, errors encountered).
fn run(cli: &Cli) -> Result<(bool, bool)> {
    let rules = load_rules(cli.config.as_deref())?;
    let compiled = compile_rules(rules)?;
    let mut dirty = false;
    let mut had_error = false;

    for path in &cli.files {
        let path_str = path.to_string_lossy().replace('\\', "/");
        let applicable: Vec<&Rule> = compiled
            .iter()
            .filter(|c| c.matches(&path_str))
            .map(|c| &c.rule)
            .collect();
        if applicable.is_empty() && !cli.well_formed {
            continue;
        }

        match process_file(cli, path, &applicable) {
            Ok(changed) => dirty |= changed,
            Err(err) => {
                eprintln!("fews-precommit: error: {err:#}");
                had_error = true;
            }
        }
    }

    Ok((dirty, had_error))
}

fn process_file(cli: &Cli, path: &std::path::Path, rules: &[&Rule]) -> Result<bool> {
    let raw = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let (bom, body) = match raw.strip_prefix(UTF8_BOM) {
        Some(rest) => (UTF8_BOM, rest),
        None => (&[][..], &raw[..]),
    };
    let content = std::str::from_utf8(body)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))?;

    let (fixed, changes) = apply_rules(content, rules)
        .with_context(|| format!("cannot process {}", path.display()))?;
    if changes.is_empty() {
        return Ok(false);
    }

    for change in &changes {
        let suffix = match change.action {
            Some(action) if !cli.check => format!(" -- {action}"),
            _ => String::new(),
        };
        println!("{}: {}{}", path.display(), change.message, suffix);
    }

    if !cli.check && fixed != content {
        let mut out = Vec::with_capacity(bom.len() + fixed.len());
        out.extend_from_slice(bom);
        out.extend_from_slice(fixed.as_bytes());
        fs::write(path, out).with_context(|| format!("cannot write {}", path.display()))?;
    }

    Ok(true)
}

fn load_rules(config_path: Option<&std::path::Path>) -> Result<Vec<Rule>> {
    let path = match config_path {
        Some(path) => path.to_path_buf(),
        None => {
            let default = PathBuf::from(DEFAULT_CONFIG_NAME);
            if !default.exists() {
                return Ok(default_rules());
            }
            default
        }
    };
    let text = fs::read_to_string(&path)
        .with_context(|| format!("cannot read config {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
    Ok(config.rules)
}

/// Rules that apply when no .fews-precommit.toml is present.
/// Note: FEWS booleans are XSD xs:boolean, so lowercase "true"/"false".
fn default_rules() -> Vec<Rule> {
    vec![Rule {
        path: "SystemConfigFiles/Explorer.xml".into(),
        tag: "adjustSystemTimeAutomatically".into(),
        expected: Some("true".into()),
        ..Rule::default()
    }]
}

fn compile_rules(rules: Vec<Rule>) -> Result<Vec<CompiledRule>> {
    rules
        .into_iter()
        .map(|rule| {
            if rule.path.is_empty() || rule.tag.is_empty() {
                bail!("every rule needs a non-empty 'path' and 'tag'");
            }
            if rule.remove {
                if rule.expected.is_some()
                    || rule.attribute.is_some()
                    || rule.required
                    || !rule.forbidden.is_empty()
                {
                    bail!(
                        "rule for <{}>: 'remove = true' cannot be combined with \
                         'expected', 'attribute', 'forbidden' or 'required'",
                        rule.tag
                    );
                }
            } else if rule.expected.is_none() {
                bail!(
                    "rule for <{}> needs 'expected' (or 'remove = true')",
                    rule.tag
                );
            }
            let matcher = Glob::new(&rule.path)
                .with_context(|| format!("invalid path glob '{}'", rule.path))?
                .compile_matcher();
            let suffix_matcher = if rule.path.starts_with("**/") {
                None
            } else {
                Some(Glob::new(&format!("**/{}", rule.path))?.compile_matcher())
            };
            Ok(CompiledRule {
                rule,
                matcher,
                suffix_matcher,
            })
        })
        .collect()
}

/// True when the element nesting (`stack`, innermost last) ends with the
/// segments of `tag`, e.g. stack [explorer, general, x] matches "general/x".
fn tag_matches(stack: &[String], tag: &str) -> bool {
    let segments: Vec<&str> = tag.split('/').collect();
    stack.len() >= segments.len()
        && stack[stack.len() - segments.len()..]
            .iter()
            .map(String::as_str)
            .eq(segments.iter().copied())
}

fn find_rule<'r>(
    stack: &[String],
    rules: &[&'r Rule],
    pred: impl Fn(&Rule) -> bool,
) -> Option<&'r Rule> {
    rules
        .iter()
        .copied()
        .find(|rule| pred(rule) && tag_matches(stack, &rule.tag))
}

fn local_name(start: &BytesStart) -> String {
    String::from_utf8_lossy(start.local_name().as_ref()).into_owned()
}

/// Applies all rules to the XML document. Only violating values are rewritten;
/// everything else (comments, whitespace, declaration) passes through untouched.
fn apply_rules(xml: &str, rules: &[&Rule]) -> Result<(String, Vec<Change>)> {
    let mut reader = Reader::from_str(xml);
    let mut writer = Writer::new(Vec::new());
    let mut changes = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut seen = vec![false; rules.len()];
    // Value rule of the innermost open element, if any.
    let mut current: Option<&Rule> = None;
    // While Some(depth): inside an element being removed; write nothing until
    // the stack drops below `depth`.
    let mut removing: Option<usize> = None;
    // Whitespace-only text is held back one event, so the indentation in
    // front of a removed element can be dropped together with it.
    let mut pending_ws: Option<Event<'static>> = None;

    loop {
        let event = reader.read_event().map_err(|err| {
            let pos = (reader.buffer_position() as usize).min(xml.len());
            let line = xml.as_bytes()[..pos]
                .iter()
                .filter(|&&b| b == b'\n')
                .count()
                + 1;
            anyhow::anyhow!("XML parse error at line {line}: {err}")
        })?;

        if let Some(depth) = removing {
            match event {
                Event::Start(ref start) => stack.push(local_name(start)),
                Event::End(_) => {
                    stack.pop();
                    if stack.len() < depth {
                        removing = None;
                    }
                }
                Event::Eof => bail!("XML is truncated inside an element being removed"),
                _ => {}
            }
            continue;
        }

        if let Event::Text(ref text) = event {
            let bytes: &[u8] = text;
            if bytes.iter().all(|b| b.is_ascii_whitespace()) {
                flush_ws(&mut writer, &mut pending_ws)?;
                pending_ws = Some(event.into_owned());
                continue;
            }
        }

        match event {
            Event::Eof => {
                flush_ws(&mut writer, &mut pending_ws)?;
                // quick-xml does not treat a premature end of file as an
                // error, but a truncated configuration file must never pass.
                if !stack.is_empty() {
                    bail!(
                        "XML is truncated: {} unclosed element(s) at end of file",
                        stack.len()
                    );
                }
                break;
            }
            Event::Start(ref start) => {
                stack.push(local_name(start));
                mark_seen(&stack, rules, &mut seen);
                current = None;

                if let Some(rule) = find_rule(&stack, rules, |r| r.remove) {
                    changes.push(Change {
                        message: format!("<{}> is not allowed", rule.tag),
                        action: Some("removed"),
                    });
                    // Drop the indentation in front of the removed element.
                    pending_ws = None;
                    removing = Some(stack.len());
                    continue;
                }

                flush_ws(&mut writer, &mut pending_ws)?;
                let (rewritten, attr_changes) = enforce_attributes(start, &stack, rules)?;
                changes.extend(attr_changes);
                current = find_rule(&stack, rules, Rule::is_value_rule);
                match rewritten {
                    Some(new_start) => writer.write_event(Event::Start(new_start))?,
                    None => writer.write_event(event)?,
                }
            }
            Event::Empty(ref empty) => {
                stack.push(local_name(empty));
                mark_seen(&stack, rules, &mut seen);

                if let Some(rule) = find_rule(&stack, rules, |r| r.remove) {
                    changes.push(Change {
                        message: format!("<{}> is not allowed", rule.tag),
                        action: Some("removed"),
                    });
                    // Drop the indentation in front of the removed element.
                    pending_ws = None;
                } else {
                    flush_ws(&mut writer, &mut pending_ws)?;
                    let (rewritten, attr_changes) = enforce_attributes(empty, &stack, rules)?;
                    changes.extend(attr_changes);
                    // An empty element where a value is enforced: fill it in,
                    // unless a forbidden list restricts the rule.
                    let fill = find_rule(&stack, rules, |r| {
                        r.is_value_rule() && r.forbidden.is_empty()
                    });
                    if let Some(rule) = fill {
                        let expected = rule.expected.as_deref().unwrap_or_default();
                        changes.push(Change {
                            message: format!("<{}> is empty, must be '{}'", rule.tag, expected),
                            action: Some("reverted"),
                        });
                        let name = String::from_utf8_lossy(empty.name().as_ref()).into_owned();
                        let start = rewritten.unwrap_or_else(|| empty.to_owned());
                        writer.write_event(Event::Start(start))?;
                        writer.write_event(Event::Text(BytesText::new(expected)))?;
                        writer.write_event(Event::End(BytesEnd::new(name)))?;
                    } else {
                        match rewritten {
                            Some(new_start) => writer.write_event(Event::Empty(new_start))?,
                            None => writer.write_event(event)?,
                        }
                    }
                }
                stack.pop();
            }
            Event::End(_) => {
                flush_ws(&mut writer, &mut pending_ws)?;
                stack.pop();
                // Recompute instead of clearing, so text following a child
                // element ("tail text") is still checked.
                current = find_rule(&stack, rules, Rule::is_value_rule);
                writer.write_event(event)?;
            }
            Event::Text(ref text) if current.is_some() => {
                flush_ws(&mut writer, &mut pending_ws)?;
                let rule = current.unwrap();
                let value = text.unescape()?;
                match fix_value(rule, &value) {
                    Some((replacement, change)) => {
                        changes.push(change);
                        writer.write_event(Event::Text(BytesText::new(&replacement)))?;
                    }
                    None => writer.write_event(event)?,
                }
            }
            Event::CData(ref cdata) if current.is_some() => {
                flush_ws(&mut writer, &mut pending_ws)?;
                let rule = current.unwrap();
                let value = std::str::from_utf8(cdata.as_ref())
                    .context("CDATA section is not valid UTF-8")?;
                match fix_value(rule, value) {
                    Some((replacement, change)) => {
                        changes.push(change);
                        writer.write_event(Event::CData(BytesCData::new(&replacement)))?;
                    }
                    None => writer.write_event(event)?,
                }
            }
            other => {
                flush_ws(&mut writer, &mut pending_ws)?;
                writer.write_event(other)?;
            }
        }
    }

    for (rule, seen) in rules.iter().zip(seen) {
        if rule.required && !seen {
            changes.push(Change {
                message: format!(
                    "required <{}> is missing (cannot be fixed automatically)",
                    rule.tag
                ),
                action: None,
            });
        }
    }

    let output = String::from_utf8(writer.into_inner())?;
    Ok((output, changes))
}

fn flush_ws(writer: &mut Writer<Vec<u8>>, pending_ws: &mut Option<Event<'static>>) -> Result<()> {
    if let Some(event) = pending_ws.take() {
        writer.write_event(event)?;
    }
    Ok(())
}

/// Checks an element value against its rule. Returns the replacement text
/// (surrounding whitespace preserved) and the change record on violation.
fn fix_value(rule: &Rule, value: &str) -> Option<(String, Change)> {
    let trimmed = value.trim();
    if trimmed.is_empty() || !rule.is_violation(trimmed) {
        return None;
    }
    let expected = rule.expected.as_deref().unwrap_or_default();
    let start = value.find(trimmed).unwrap_or_default();
    let replacement = format!(
        "{}{}{}",
        &value[..start],
        expected,
        &value[start + trimmed.len()..],
    );
    let change = Change {
        message: format!("<{}> is '{}', must be '{}'", rule.tag, trimmed, expected),
        action: Some("reverted"),
    };
    Some((replacement, change))
}

fn mark_seen(stack: &[String], rules: &[&Rule], seen: &mut [bool]) {
    for (index, rule) in rules.iter().enumerate() {
        if tag_matches(stack, &rule.tag) {
            seen[index] = true;
        }
    }
}

/// Enforces attribute rules on a start/empty tag. Returns a rewritten tag when
/// anything changed. Note: rewriting normalizes whitespace between attributes.
fn enforce_attributes(
    element: &BytesStart,
    stack: &[String],
    rules: &[&Rule],
) -> Result<(Option<BytesStart<'static>>, Vec<Change>)> {
    let attr_rules: Vec<&Rule> = rules
        .iter()
        .copied()
        .filter(|rule| !rule.remove && rule.attribute.is_some() && tag_matches(stack, &rule.tag))
        .collect();
    if attr_rules.is_empty() {
        return Ok((None, Vec::new()));
    }

    let mut changes = Vec::new();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let mut found = vec![false; attr_rules.len()];
    let mut changed = false;

    for attribute in element.attributes() {
        let attribute = attribute.context("malformed attribute")?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let mut value = attribute.unescape_value()?.into_owned();
        for (index, rule) in attr_rules.iter().enumerate() {
            if rule.attribute.as_deref() == Some(key.as_str()) {
                found[index] = true;
                if rule.is_violation(&value) {
                    let expected = rule.expected.as_deref().unwrap_or_default();
                    changes.push(Change {
                        message: format!(
                            "<{}> attribute {}=\"{}\", must be \"{}\"",
                            rule.tag, key, value, expected
                        ),
                        action: Some("reverted"),
                    });
                    value = expected.to_string();
                    changed = true;
                }
            }
        }
        attributes.push((key, value));
    }

    // Add attributes that are missing entirely, unless a forbidden list
    // restricts the rule to specific wrong values.
    for (index, rule) in attr_rules.iter().enumerate() {
        if !found[index] && rule.forbidden.is_empty() {
            let key = rule.attribute.clone().unwrap_or_default();
            let expected = rule.expected.clone().unwrap_or_default();
            changes.push(Change {
                message: format!(
                    "<{}> is missing attribute {}=\"{}\"",
                    rule.tag, key, expected
                ),
                action: Some("added"),
            });
            attributes.push((key, expected));
            changed = true;
        }
    }

    if !changed {
        return Ok((None, changes));
    }
    let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
    let mut rewritten = BytesStart::new(name);
    for (key, value) in &attributes {
        rewritten.push_attribute((key.as_str(), value.as_str()));
    }
    Ok((Some(rewritten), changes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tag: &str, expected: &str) -> Rule {
        Rule {
            path: "**/*.xml".into(),
            tag: tag.into(),
            expected: Some(expected.into()),
            ..Rule::default()
        }
    }

    #[test]
    fn reverts_debug_value_to_default() {
        let xml = "<explorer>\r\n  <adjustSystemTimeAutomatically>False</adjustSystemTimeAutomatically>\r\n</explorer>";
        let r = rule("adjustSystemTimeAutomatically", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].message.contains("'False'"));
        assert_eq!(
            fixed,
            "<explorer>\r\n  <adjustSystemTimeAutomatically>True</adjustSystemTimeAutomatically>\r\n</explorer>"
        );
    }

    #[test]
    fn leaves_correct_value_untouched() {
        let xml = "<explorer><adjustSystemTimeAutomatically>True</adjustSystemTimeAutomatically></explorer>";
        let r = rule("adjustSystemTimeAutomatically", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert!(changes.is_empty());
        assert_eq!(fixed, xml);
    }

    #[test]
    fn preserves_comments_attributes_and_declaration() {
        let xml = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!-- FEWS explorer settings -->\n",
            "<explorer xmlns=\"http://www.wldelft.nl/fews\" version=\"1.0\">\n",
            "  <debug enabled=\"yes\">False</debug>\n",
            "</explorer>\n",
        );
        let r = rule("debug", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, xml.replace(">False<", ">True<"));
    }

    #[test]
    fn forbidden_list_limits_what_is_reverted() {
        let xml = "<a><logLevel>custom</logLevel></a>";
        let mut r = rule("logLevel", "INFO");
        r.forbidden = vec!["DEBUG".into(), "TRACE".into()];
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert!(changes.is_empty(), "'custom' is not in the forbidden list");
        assert_eq!(fixed, xml);

        let xml = "<a><logLevel>DEBUG</logLevel></a>";
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><logLevel>INFO</logLevel></a>");
    }

    #[test]
    fn only_named_tag_is_touched() {
        let xml = "<a><other>False</other><target>False</target></a>";
        let r = rule("target", "True");
        let (fixed, _) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(fixed, "<a><other>False</other><target>True</target></a>");
    }

    #[test]
    fn tag_path_disambiguates_nested_elements() {
        let xml = "<a><general><x>1</x></general><other><x>1</x></other></a>";
        let r = rule("general/x", "2");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            fixed,
            "<a><general><x>2</x></general><other><x>1</x></other></a>"
        );
    }

    #[test]
    fn ignore_case_treats_value_as_equal() {
        let xml = "<a><flag>TRUE</flag></a>";
        let mut r = rule("flag", "true");
        r.ignore_case = true;
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert!(changes.is_empty());
        assert_eq!(fixed, xml);

        let xml = "<a><flag>False</flag></a>";
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><flag>true</flag></a>");
    }

    #[test]
    fn attribute_value_is_enforced() {
        let xml = "<a>\n  <export debug=\"true\" name=\"x\">data</export>\n</a>";
        let mut r = rule("export", "false");
        r.attribute = Some("debug".into());
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            fixed,
            "<a>\n  <export debug=\"false\" name=\"x\">data</export>\n</a>"
        );
    }

    #[test]
    fn missing_attribute_is_added_unless_forbidden_list_given() {
        let xml = "<a><export name=\"x\"/></a>";
        let mut r = rule("export", "false");
        r.attribute = Some("debug".into());
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><export name=\"x\" debug=\"false\"/></a>");

        // With a forbidden list, a missing attribute is fine.
        r.forbidden = vec!["true".into()];
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert!(changes.is_empty());
        assert_eq!(fixed, xml);
    }

    #[test]
    fn remove_rule_deletes_element_and_indentation() {
        let xml = "<a>\n\t<debugSettings>\n\t\t<level>ALL</level>\n\t</debugSettings>\n\t<keep>x</keep>\n</a>";
        let r = Rule {
            path: "**/*.xml".into(),
            tag: "debugSettings".into(),
            remove: true,
            ..Rule::default()
        };
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a>\n\t<keep>x</keep>\n</a>");
    }

    #[test]
    fn remove_rule_deletes_self_closing_element() {
        let xml = "<a>\n\t<showDebugPanel/>\n\t<keep>x</keep>\n</a>";
        let r = Rule {
            path: "**/*.xml".into(),
            tag: "showDebugPanel".into(),
            remove: true,
            ..Rule::default()
        };
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a>\n\t<keep>x</keep>\n</a>");
    }

    #[test]
    fn required_element_missing_is_reported_as_unfixable() {
        let xml = "<a><other>1</other></a>";
        let mut r = rule("mustExist", "x");
        r.required = true;
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].message.contains("missing"));
        assert!(changes[0].action.is_none());
        assert_eq!(fixed, xml);
    }

    #[test]
    fn empty_element_is_filled_with_expected_value() {
        let xml = "<a><target/></a>";
        let r = rule("target", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><target>True</target></a>");
    }

    #[test]
    fn tail_text_after_child_element_is_checked() {
        let xml = "<a><target><note>n</note>False</target></a>";
        let r = rule("target", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><target><note>n</note>True</target></a>");
    }

    #[test]
    fn cdata_value_is_enforced() {
        let xml = "<a><target><![CDATA[False]]></target></a>";
        let r = rule("target", "True");
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a><target><![CDATA[True]]></target></a>");
    }

    #[test]
    fn removing_last_child_keeps_closing_tag_on_own_line() {
        let xml = "<a>\n\t<keep>x</keep>\n\t<debug>z</debug>\n</a>";
        let r = Rule {
            path: "**/*.xml".into(),
            tag: "debug".into(),
            remove: true,
            ..Rule::default()
        };
        let (fixed, changes) = apply_rules(xml, &[&r]).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(fixed, "<a>\n\t<keep>x</keep>\n</a>");
    }

    #[test]
    fn parse_errors_report_line_numbers() {
        let r = rule("target", "True");
        let err = apply_rules("<a>\n<b>\n</c>\n</a>", &[&r]).unwrap_err();
        assert!(err.to_string().contains("line 3"), "got: {err:#}");
    }

    #[test]
    fn rules_with_empty_path_or_tag_are_rejected() {
        let no_tag = Rule {
            path: "**/*.xml".into(),
            expected: Some("v".into()),
            ..Rule::default()
        };
        assert!(compile_rules(vec![no_tag]).is_err());

        let no_path = Rule {
            tag: "x".into(),
            expected: Some("v".into()),
            ..Rule::default()
        };
        assert!(compile_rules(vec![no_path]).is_err());
    }

    #[test]
    fn truncated_xml_is_rejected() {
        let r = rule("target", "True");
        assert!(apply_rules("<a><target>False</target>", &[&r]).is_err());
        assert!(apply_rules("<a><b></c></a>", &[&r]).is_err());
    }

    #[test]
    fn rules_without_expected_or_remove_are_rejected() {
        let bad = Rule {
            path: "**/*.xml".into(),
            tag: "x".into(),
            ..Rule::default()
        };
        assert!(compile_rules(vec![bad]).is_err());

        let conflicting = Rule {
            path: "**/*.xml".into(),
            tag: "x".into(),
            expected: Some("y".into()),
            remove: true,
            ..Rule::default()
        };
        assert!(compile_rules(vec![conflicting]).is_err());
    }

    #[test]
    fn relative_pattern_matches_nested_paths() {
        let compiled =
            compile_rules(vec![rule_with_path("SystemConfigFiles/Explorer.xml")]).unwrap();
        assert!(compiled[0].matches("SystemConfigFiles/Explorer.xml"));
        assert!(compiled[0].matches("Config/SystemConfigFiles/Explorer.xml"));
        assert!(!compiled[0].matches("SystemConfigFiles/Other.xml"));
    }

    fn rule_with_path(path: &str) -> Rule {
        Rule {
            path: path.into(),
            tag: "t".into(),
            expected: Some("v".into()),
            ..Rule::default()
        }
    }

    #[test]
    fn default_rules_cover_explorer_example() {
        let compiled = compile_rules(default_rules()).unwrap();
        assert!(compiled[0].matches("SystemConfigFiles/Explorer.xml"));
        let xml = "<explorer><adjustSystemTimeAutomatically>false</adjustSystemTimeAutomatically></explorer>";
        let (fixed, changes) = apply_rules(xml, &[&compiled[0].rule]).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(fixed.contains(">true<"));
    }
}
