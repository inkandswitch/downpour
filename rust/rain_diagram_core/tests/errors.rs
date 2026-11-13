//! Error messages for the mistakes people actually make, and the input
//! formats editors actually produce. The messages are part of the UX, so
//! they're pinned here word for word, with their positions.

use rain_diagram_core::{edit, parse};

/// `line:column: message` for `source`'s error.
fn error(source: &str) -> String {
    match parse(source) {
        Ok(_) => String::from("parsed"),
        Err(e) => {
            let (line, column) = e.span().line_col(source);
            format!("{line}:{column}: {e}")
        }
    }
}

#[test]
fn common_mistakes_say_what_to_do() {
    for (source, expected) in [
        ("", "1:1: missing header; start with `rain` or `certflow`"),
        (
            "(A)-[read(A)]->(B)",
            "1:1: unknown diagram kind \"(A)-[read(A)]->(B)\"; expected `rain` or `certflow`",
        ),
        (
            "rain\n(A)-[write]->(B)",
            "2:6: unknown access level \"write\"; expected admin, edit, read, relay, or a command path like /crud/read",
        ),
        (
            "rain\n(A)-[read]->(B)",
            "2:10: expected the subject after the ability, as in `[edit(Doc)]`",
        ),
        (
            "rain\n(A)-[read(A)->(B)",
            "2:13: expected `]` to close the certificate",
        ),
        (
            "rain\n(A-[read]->(B)",
            "2:4: expected `)` to close the principal",
        ),
        (
            "rain\n(A)-[read(A)]-(B)",
            "2:15: expected `->` after the certificate",
        ),
        (
            "rain\n(A)-[read(A)]->(B)\n(A)-<revoke>-x[a-b]",
            "3:1: no earlier certificate is labelled `a-b`; write `a-b: ` before the one you mean",
        ),
        (
            "rain\n(A)-[/crud/read where n < 3]->(B)",
            "2:17: expected the subject after the ability, as in `[edit(Doc)]`",
        ),
        (
            "rain\nx: (A)=[[read]]=>(B)",
            "2:1: only delegations can be labelled",
        ),
        (
            "rain\n>>> A\n(A)-[read(A)]->(B)",
            "2:1: region `A` is never closed; add `<<< A`",
        ),
        (
            "rain\n(A)-[read(A)]->(B) @60%",
            "2:20: expected end of line, or `:` before a note",
        ),
    ] {
        assert_eq!(error(source), expected, "{source:?}");
    }
}

/// Some editors start UTF-8 files with a byte-order mark.
#[test]
fn a_byte_order_mark_is_ignored() -> Result<(), rain_diagram_core::Error> {
    let with = parse("\u{feff}rain\n(A)-[read(A)]->(B)")?;
    let without = parse("rain\n(A)-[read(A)]->(B)")?;
    assert_eq!(with.to_text(), without.to_text());

    // Errors still point at the right place in the original text
    let source = "\u{feff}rain\n(A)-[write]->(B)";
    assert_eq!(
        error(source),
        "2:6: unknown access level \"write\"; expected admin, edit, read, relay, or a command path like /crud/read"
    );
    Ok(())
}

#[test]
fn line_endings() -> Result<(), Box<dyn std::error::Error>> {
    // CRLF and mixed endings parse the same as LF
    let lf = parse("rain\n(A)-[read(A)]->(B) : note\n(B)=[[read]]=>(A)\n")?;
    for source in [
        "rain\r\n(A)-[read(A)]->(B) : note\r\n(B)=[[read]]=>(A)\r\n",
        "rain\n(A)-[read(A)]->(B) : note\r\n(B)=[[read]]=>(A)\n",
    ] {
        assert_eq!(parse(source)?.to_text(), lf.to_text(), "{source:?}");
    }

    // Classic Mac `\r`-only endings aren't line breaks: one long first line,
    // refused rather than misread
    assert!(parse("rain\r(A)-[read(A)]->(B)").is_err());

    // Edits keep each line's own ending
    let mixed = "rain\r\n(A)-[read(A)]->(B)\n(C)-[read(C)]->(D)\r\n";
    assert_eq!(
        edit::move_statement(mixed, 0, 1)?,
        "rain\r\n(C)-[read(C)]->(D)\r\n(A)-[read(A)]->(B)\n"
    );
    assert_eq!(
        edit::set_position(mixed, 1, Some(75))?,
        "rain\r\n(A)-[read(A)]->(B)\n(C)---[read(C)]->(D)\r\n"
    );
    Ok(())
}
