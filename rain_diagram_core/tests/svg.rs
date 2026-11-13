//! The SVG's structure: what the interactive embed and stylesheets rely on.
//! Coordinates aren't pinned; hooks, classes, and wiring are.

use rain_diagram_core::parse;
use testresult::TestResult;

/// The opening tag of the first element containing `needle`.
fn tag_with<'a>(svg: &'a str, needle: &str) -> Option<&'a str> {
    let at = svg.find(needle)?;
    let start = svg[..at].rfind('<')?;
    let end = at + svg[at..].find('>')?;
    svg.get(start..=end)
}

const REVOCATION: &str = "rain
(Doc)-[edit]->(Alice)
(Alice)-[edit]->(Bob)
(Bob)=[[edit]]=>(Doc)
(Bob)-[read]->(Carol)
(Carol)-[read]->(Dan)
(Alice)-<revoke>-x[carol-dan]";

#[test]
fn every_event_is_hooked_with_its_status() -> TestResult {
    let svg = parse(REVOCATION)?.to_svg();

    assert!(tag_with(&svg, r#"data-cert="doc-alice""#).is_some_and(|t| {
        t.starts_with(r#"<g class="event delegate valid" data-event="0" data-cert="doc-alice""#)
    }));
    assert!(tag_with(&svg, r#"data-cert="carol-dan""#).is_some_and(|t| {
        t.starts_with(r#"<g class="event delegate revoked" data-event="4" data-cert="carol-dan""#)
    }));
    assert!(svg.contains(r#"<g class="event invoke valid" data-event="2""#));
    assert!(svg.contains(r#"<g class="event revoke valid" data-event="5""#));
    for key in ["doc", "alice", "bob", "carol", "dan"] {
        assert!(svg.contains(&format!(r#"<g class="principal" data-principal="{key}""#)));
    }
    Ok(())
}

#[test]
fn powerlines_follow_routes() -> TestResult {
    let svg = parse(REVOCATION)?.to_svg();
    let wires: Vec<(Option<&str>, &str)> = svg
        .match_indices(r#"<path class="powerline""#)
        .filter_map(|(at, _)| {
            let tag = &svg[at..at + svg[at..].find('>')?];
            let attr = |name: &str| {
                let start = tag.find(&format!(r#" {name}=""#))? + name.len() + 3;
                tag.get(start..start + tag[start..].find('"')?)
            };
            Some((attr("data-from"), attr("data-to")?))
        })
        .collect();

    assert_eq!(
        wires,
        [
            (None, "0"),      // the Doc's own authority, off its lifeline
            (Some("0"), "1"), // Doc→Alice powers Alice→Bob
            (Some("1"), "2"), // …and Bob's edit
            (Some("1"), "3"), // …and Bob→Carol
            (Some("3"), "4"), // Bob→Carol powers Carol→Dan
            (Some("0"), "5"), // Alice revokes with what flows through her
        ]
    );
    Ok(())
}

#[test]
fn colours_follow_subjects_and_revocations_are_red() -> TestResult {
    let svg = parse(REVOCATION)?.to_svg();
    // Every powerline is about the Doc (first principal: blue), except the
    // revocation's
    assert!(svg.matches(r##"stroke="#3d7dd8"/>"##).count() >= 5);
    assert!(tag_with(&svg, r#"data-to="5""#).is_some_and(|t| t.contains(r##"stroke="#d33""##)));
    Ok(())
}

#[test]
fn void_and_unauthorized_are_marked() -> TestResult {
    let svg = parse(
        "rain
         (Doc)-[admin]->(Alice)
         (Alice)-[edit(Doc)]->(Bob)
         (Bob)-[edit(Doc)]->(Carol)
         (Carol)=[[edit]]=>(Doc)
         (Alice)-<revoke>-x[alice-bob]
         (Eve)=[[edit]]=>(Doc)",
    )?
    .to_svg();
    assert!(svg.contains(r#"<g class="event delegate void" data-event="2""#));
    assert!(svg.contains(r#"<g class="event invoke void" data-event="3""#));
    assert!(svg.contains(r#"<g class="event invoke unauthorized" data-event="5""#));
    Ok(())
}

#[test]
fn transitive_handles_are_drawn() -> TestResult {
    let svg = parse(
        "rain
         (Doc)-[edit]->(Team)
         (Team)-[admin]->(Alice)
         (Alice)-[edit(Doc)]->(Bob)",
    )?
    .to_svg();
    assert!(svg.contains(r#"class="handle""#));
    assert!(tag_with(&svg, r#"class="handle""#).is_some_and(|t| t.contains(r#"data-event="1""#)));
    // Doc's authority runs into Team→Alice's handle, then out to Alice→Bob
    assert!(svg.contains(r#"data-from="0" data-to="1""#));
    assert!(svg.contains(r#"data-from="1" data-to="2""#));
    Ok(())
}

#[test]
fn titles_and_notes_widen_the_picture() -> TestResult {
    let plain = parse("rain\n(A)-[read]->(B)")?.to_svg();
    let noted = parse("rain\ntitle T\n(A)-[read]->(B) : a rather long note about this")?.to_svg();
    let width = |svg: &str| -> Option<f64> {
        let start = svg.find(r#"width=""#)? + 7;
        svg[start..start + svg[start..].find('"')?].parse().ok()
    };
    assert!(width(&noted) > width(&plain));
    assert!(noted.contains(r#"<text class="title""#));
    assert!(noted.contains(">a rather long note about this</text>"));
    assert!(!plain.contains(r#"class="title""#));
    Ok(())
}

/// Authority over yourself starts on your lifeline: from the head on the
/// first row, from a small circle after that.
#[test]
fn roots_start_on_the_issuers_lifeline() -> TestResult {
    let svg = parse(
        "rain
         (Doc)-[edit]->(Alice)
         (Team)-[admin]->(Alice)",
    )?
    .to_svg();
    assert_eq!(svg.matches(r#"<circle class="root""#).count(), 1);
    assert!(
        tag_with(&svg, r#"<circle class="root""#).is_some_and(|t| t.contains(r#"data-event="1""#))
    );
    Ok(())
}

#[test]
fn certflow_has_the_same_hooks_without_invocations() -> TestResult {
    let d = parse(REVOCATION)?;
    let svg = d.to_certflow_svg();
    assert!(svg.contains(r#"class="rain-diagram certflow""#));
    assert!(
        svg.contains(r#"<g class="event delegate revoked" data-event="4" data-cert="carol-dan">"#)
    );
    assert!(svg.contains(r#"<g class="event revoke valid" data-event="5""#));
    assert!(
        !svg.contains(r#"class="event invoke"#),
        "certflow has no invocations"
    );

    // The header picks the notation
    assert_eq!(
        parse(&REVOCATION.replacen("rain", "certflow", 1))?.to_svg(),
        svg
    );
    assert!(
        d.to_svg()
            .starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg" class="rain-diagram" "#)
    );
    Ok(())
}

#[test]
fn certflow_powerlines_hop_between_certificates() -> TestResult {
    let svg = parse(REVOCATION)?.to_certflow_svg();
    let wires: Vec<_> = svg
        .match_indices(r#"<path class="powerline""#)
        .filter_map(|(at, _)| svg.get(at..at + svg[at..].find(" d=")?))
        .map(|tag| tag.trim_start_matches(r#"<path class="powerline""#).trim())
        .collect();
    assert_eq!(
        wires,
        [
            r#"data-to="0""#,
            r#"data-from="0" data-to="1""#,
            r#"data-from="1" data-to="3""#,
            r#"data-from="3" data-to="4""#,
            r#"data-from="0" data-to="5""#,
        ]
    );
    Ok(())
}

#[test]
fn regions_are_hooked_boxes_in_both_notations() -> TestResult {
    let d = parse(
        "rain
         (Doc)-[edit]->(Alice)
         >>> A: Alice <does> stuff
         (Alice)-[edit]->(Bob)
         >>> B
         (Bob)=[[edit]]=>(Doc)
         <<< B
         <<< A",
    )?;
    for svg in [d.to_rain_svg(), d.to_certflow_svg()] {
        assert!(svg.contains(r#"<g class="region" data-region="A""#));
        assert!(
            svg.contains(">Alice &lt;does&gt; stuff</text>"),
            "captions are escaped"
        );
    }
    // B only holds an invocation, which certflow doesn't draw
    assert!(
        d.to_rain_svg()
            .contains(r#"<g class="region" data-region="B">"#)
    );
    assert!(!d.to_certflow_svg().contains(r#"data-region="B""#));
    Ok(())
}

#[test]
fn regions_can_take_a_principals_colour() -> TestResult {
    let d = parse(
        "rain
         (Doc)-[edit]->(Alice)
         (Alice)-[edit]->(Bob)
         >>> A (Bob): Bob does stuff
         (Bob)=[[edit]]=>(Doc)
         <<< A",
    )?;
    // Bob is the third principal: green
    let svg = d.to_rain_svg();
    let tag = svg
        .find(r#"<rect class="region-box""#)
        .and_then(|at| svg.get(at..at + svg[at..].find("/>")?));
    assert!(
        tag.is_some_and(|t| t.contains(r##"stroke="#3f9b5a""##)),
        "{tag:?}"
    );

    assert!(matches!(
        parse("rain\n(A)-[read]->(B)\n>>> A (Zed)\n(B)-[read]->(C)\n<<< A"),
        Err(rain_diagram_core::Error::Resolve(e))
            if matches!(e.kind(), rain_diagram_core::diagram::ResolveErrorKind::UnknownPrincipal(_))
    ));
    Ok(())
}

/// Routes of different colours through one membership get a bead each,
/// side by side; routes of the same colour share one.
#[test]
fn handles_through_one_membership_never_overlap() -> TestResult {
    let svg = parse(
        "rain
         (Doc)-[admin]->(Team)
         (Team)-[admin]->(Eve)
         (Eve)=[[edit]]=>(Doc)
         (Eve)=[[read]]=>(Doc)
         (Doc)-[read]->(Bob)
         (Eve)-<revoke>-x[doc-bob]",
    )?
    .to_rain_svg();

    let handles: Vec<(&str, &str)> = svg
        .match_indices(r#"<circle class="handle""#)
        .filter_map(|(at, _)| svg.get(at..at + svg[at..].find("/>")?))
        .filter(|tag| tag.contains(r#"data-event="1""#))
        .filter_map(|tag| {
            let attr = |name: &str| {
                let from = tag.find(&format!(r#"{name}=""#))? + name.len() + 2;
                tag.get(from..from + tag[from..].find('"')?)
            };
            Some((attr("cx")?, attr("stroke")?))
        })
        .collect();

    // One Doc-blue bead for both invocations, one red for the revocation
    assert_eq!(handles.len(), 2, "{handles:?}");
    assert_ne!(handles[0].0, handles[1].0, "{handles:?}");
    assert_ne!(handles[0].1, handles[1].1, "{handles:?}");
    Ok(())
}

/// Inline SVG `<style>` is parsed as markup when a page inserts the SVG with
/// `innerHTML`, so any `<` would cut the stylesheet short.
#[test]
fn stylesheet_survives_inline_parsing() -> TestResult {
    let svg = parse("rain\n(A)-[read]->(B)")?.to_svg();
    let css = svg
        .split("<style>")
        .nth(1)
        .and_then(|rest| rest.split("</style>").next())
        .unwrap_or_default();
    assert!(!css.is_empty());
    assert!(!css.contains('<'), "{css}");
    Ok(())
}
