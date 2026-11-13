//! Property tests for the parser and resolver.

mod common;

use bolero::check;
use rain_diagram_core::{ast::Document, parser};

/// Printing a document and parsing it back gives the same document.
#[test]
fn display_parse_roundtrip() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(doc) = common::arbitrary::<Document>(bytes) else {
            return;
        };
        let doc = &doc;
        let source = doc.to_string();
        let reparsed = parser::document(&source);
        assert!(
            reparsed.is_ok(),
            "printed source should parse: {reparsed:?}\n{source}"
        );
        let Ok(reparsed) = reparsed else { return };

        assert_eq!(reparsed.kind, doc.kind);
        assert_eq!(
            reparsed.title.as_ref().map(|t| &t.node),
            doc.title.as_ref().map(|t| &t.node),
            "{source}"
        );
        assert!(
            reparsed
                .participants
                .iter()
                .map(|p| &p.node)
                .eq(doc.participants.iter().map(|p| &p.node)),
            "{source}"
        );
        assert!(
            reparsed
                .statements
                .iter()
                .map(|s| &s.node)
                .eq(doc.statements.iter().map(|s| &s.node)),
            "{source}"
        );
        assert!(
            reparsed
                .regions
                .iter()
                .map(|r| &r.node)
                .eq(doc.regions.iter().map(|r| &r.node)),
            "{source}"
        );
    });
}

/// Arbitrary input never panics, and every error span lies within the source
/// on character boundaries.
#[test]
fn never_panics_and_spans_are_in_bounds() {
    check!().with_type::<String>().for_each(|source| {
        if let Err(e) = rain_diagram_core::parse(source) {
            let span = e.span();
            assert!(span.start <= span.end && span.end <= source.len());
            assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
        }
    });
}

/// Every span on a parsed statement covers exactly its source line.
#[test]
fn statement_spans_cover_their_lines() {
    check!().with_type::<Vec<u8>>().for_each(|bytes| {
        let Some(doc) = common::arbitrary::<Document>(bytes) else {
            return;
        };
        let doc = &doc;
        let source = doc.to_string();
        if let Ok(reparsed) = parser::document(&source) {
            for (statement, original) in reparsed.statements.iter().zip(&doc.statements) {
                assert_eq!(
                    source.get(statement.span.start..statement.span.end),
                    Some(original.node.to_string().as_str())
                );
            }
        }
    });
}
