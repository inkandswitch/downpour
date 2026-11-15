//! The agent skill's examples (`plugins/rain-diagram/skills/`) are diagrams
//! an agent will copy, so each must say what the skill says it does: a
//! ` ```rain ` or ` ```certflow ` block resolves with no warnings, and one
//! tagged ` ```rain warns ` resolves with at least one.

use rain_diagram_cli::render::{Severity, parse_as};
use rain_diagram_core::ast::Kind;
use std::{fs, path::Path};
use testresult::TestResult;

struct Block {
    kind: Kind,
    line: usize,
    source: String,
    warns: bool,
}

fn blocks(markdown: &str) -> Vec<Block> {
    let mut found = Vec::new();
    let mut open: Option<Block> = None;
    for (index, line) in markdown.lines().enumerate() {
        let fence = line.trim_start();
        match &mut open {
            None => {
                let mut info = fence
                    .strip_prefix("```")
                    .unwrap_or_default()
                    .split_whitespace();
                let kind = match info.next() {
                    Some("rain") => Kind::Rain,
                    Some("certflow") => Kind::Certflow,
                    _ => continue,
                };
                open = Some(Block {
                    kind,
                    line: index + 1,
                    source: String::new(),
                    warns: info.any(|word| word == "warns"),
                });
            }
            Some(_) if fence == "```" => found.extend(open.take()),
            Some(block) => {
                block.source.push_str(line);
                block.source.push('\n');
            }
        }
    }
    found
}

#[test]
fn skill_examples_say_what_they_show() -> TestResult {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/rain-diagram/skills");
    let mut checked = 0;
    for skill in fs::read_dir(&dir)? {
        let path = skill?.path().join("SKILL.md");
        let name = path.display().to_string();
        for block in blocks(&fs::read_to_string(&path)?) {
            let (diagram, mut problems) = parse_as(&name, &block.source, Some(block.kind));
            for p in &mut problems {
                p.line += block.line;
            }
            let messages: Vec<String> = problems.iter().map(ToString::to_string).collect();
            let at = format!("{name}:{}", block.line);
            assert!(diagram.is_some(), "{at}: doesn't resolve: {messages:#?}");
            let warned = problems.iter().any(|p| p.severity == Severity::Warning);
            assert_eq!(
                warned, block.warns,
                "{at}: tagged `warns` = {}, but its warnings are {messages:#?}",
                block.warns
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 4,
        "only {checked} examples found under {}",
        dir.display()
    );
    Ok(())
}
