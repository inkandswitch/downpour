//! The binaries end to end: arguments, stdin and stdout, files, exit codes.

use std::{
    io::Write,
    process::{Command, Output, Stdio},
};
use testresult::TestResult;

const DOC: &str = "rain\n(Doc)-[edit(Doc)]->(Alice)\n(Alice)-[read(Doc)]->(Bob)\n";

fn run(bin: &str, args: &[&str], stdin: &str) -> TestResult<Output> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin is piped")?
        .write_all(stdin.as_bytes())?;
    Ok(child.wait_with_output()?)
}

fn cli(args: &[&str], stdin: &str) -> TestResult<Output> {
    run(env!("CARGO_BIN_EXE_rain-diagram"), args, stdin)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn renders_stdin_to_each_format() -> TestResult {
    let art = cli(&["render"], DOC)?;
    assert!(art.status.success());
    assert!(
        text(&art.stdout).starts_with(" (Doc)"),
        "{}",
        text(&art.stdout)
    );

    let svg = cli(
        &["render", "--format", "certflow-svg", "--at", "1", "-"],
        DOC,
    )?;
    assert!(text(&svg.stdout).starts_with("<svg"));
    assert!(text(&svg.stdout).contains("certflow"));

    let json = cli(&["render", "-f", "json"], DOC)?;
    assert!(text(&json.stdout).starts_with(r#"{"agents""#));
    Ok(())
}

#[test]
fn the_output_extension_picks_the_format() -> TestResult {
    let dir = std::env::temp_dir().join(format!("rain-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let (input, output) = (dir.join("doc.rain"), dir.join("doc.svg"));
    std::fs::write(&input, DOC)?;

    let done = cli(
        &[
            "render",
            &input.display().to_string(),
            "-o",
            &output.display().to_string(),
        ],
        "",
    )?;
    let svg = std::fs::read_to_string(&output);

    // A .certflow file can leave out its header
    let snapshot = dir.join("doc.certflow");
    std::fs::write(&snapshot, "(Doc)-[edit(Doc)]->(Alice)\n")?;
    let certflow = cli(
        &["render", "-f", "svg", &snapshot.display().to_string()],
        "",
    )?;

    std::fs::remove_dir_all(&dir)?;
    assert!(done.status.success(), "{}", text(&done.stderr));
    assert!(svg?.starts_with("<svg"));
    assert!(certflow.status.success(), "{}", text(&certflow.stderr));
    assert!(text(&certflow.stdout).contains("rain-diagram certflow"));
    Ok(())
}

#[test]
fn problems_are_reported_where_they_are() -> TestResult {
    let broken = cli(&["render"], "rain\n(A)-[read]->(B)\n")?;
    assert_eq!(broken.status.code(), Some(1));
    assert!(broken.stdout.is_empty());
    assert_eq!(
        text(&broken.stderr),
        "<stdin>:2:10: error: expected the subject after the ability, as in `[edit(Doc)]`\n"
    );

    // A warning still renders, and only fails a check with --deny-warnings
    let warned = "rain\n(A)-[edit(Doc)]->(B)\n";
    let rendered = cli(&["render"], warned)?;
    assert!(rendered.status.success());
    assert!(text(&rendered.stderr).contains("<stdin>:2:1: warning: A holds no authority over Doc"));
    assert!(cli(&["check", "-"], warned)?.status.success());
    assert_eq!(
        cli(&["check", "--deny-warnings", "-"], warned)?
            .status
            .code(),
        Some(1)
    );

    let missing = cli(&["check", "no/such/file.rain"], "")?;
    assert_eq!(missing.status.code(), Some(1));
    assert!(text(&missing.stderr).starts_with("rain-diagram: can't read no/such/file.rain"));

    assert_eq!(cli(&["render", "--at", "1"], DOC)?.status.code(), Some(1));
    assert_eq!(
        cli(&["render", "--format", "png"], DOC)?.status.code(),
        Some(2)
    );
    Ok(())
}

#[test]
fn the_preprocessor_speaks_mdbooks_protocol() -> TestResult {
    let bin = env!("CARGO_BIN_EXE_mdbook-rain-diagram");
    assert!(run(bin, &["supports", "html"], "")?.status.success());
    assert!(run(bin, &["supports", "markdown"], "")?.status.success());

    let input = r#"[{"root":".","config":{},"renderer":"html","mdbook_version":"0.5.2"},
        {"items":[{"Chapter":{"name":"A","content":"```rain\n(Doc)-[edit(Doc)]->(Alice)\n```\n",
        "number":[1],"sub_items":[],"path":"a.md","source_path":"a.md","parent_names":[]}}]}]"#;
    let out = run(bin, &[], input)?;
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains(r#"<div class=\"rain-diagram\""#));

    assert_eq!(run(bin, &[], "not json")?.status.code(), Some(1));
    Ok(())
}
