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
carol-dan: (Carol)-[read]->(Dan)
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
        .match_indices(r#"<path class="powerline "#)
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
         alice-bob: (Alice)-[edit(Doc)]->(Bob)
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
        .match_indices(r#"<path class="powerline "#)
        .filter_map(|(at, _)| svg.get(at..at + svg[at..].find(" d=")?))
        // Past the class (which carries a status), to the hooks
        .map(|tag| {
            tag.split_once(r#"" "#)
                .map_or(tag, |(_, hooks)| hooks)
                .trim()
        })
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
         doc-bob: (Doc)-[read]->(Bob)
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

/// Authority that reaches a membership after it was signed joins on the
/// member's lifeline below its source, rather than climbing back up to the
/// membership's box.
#[test]
fn late_authority_joins_on_the_members_lifeline() -> TestResult {
    let svg = parse(
        "rain
         zoe-alice: (Zoe)-[admin]->(Alice)
         (Doc)-[read]->(Zoe)
         (Alice)-[read(Doc)]->(Bob)",
    )?
    .to_rain_svg();

    let attr = |tag: &str, name: &str| -> Option<f64> {
        let from = tag.find(&format!(r#" {name}=""#))? + name.len() + 3;
        tag.get(from..from + tag[from..].find('"')?)?.parse().ok()
    };
    let handle = tag_with(&svg, r#"<circle class="handle""#).ok_or("a handle")?;
    let after = svg
        .find(r#"data-event="1""#)
        .and_then(|at| tag_with(&svg[at..], r#"class="pill""#))
        .ok_or("event 1's pill")?;

    assert!(svg.contains("<title>through zoe-alice</title>"), "{svg}");
    // On Alice's lifeline, and below Zoe's Read (where the authority comes from)
    let alice = svg
        .find(r#"data-principal="alice""#)
        .and_then(|at| attr(&svg[at..], "data-x"));
    assert_eq!(attr(handle, "cx"), alice);
    assert!(attr(handle, "cy") > attr(after, "y"), "{handle} vs {after}");
    Ok(())
}

/// A revocation that has no effect mustn't look like it cut anything: the
/// effective one strikes its target, the ineffective one doesn't, in both
/// the SVG and the text art.
#[test]
fn only_effective_revocations_strike_their_target() -> TestResult {
    let effective = parse("rain\nab: (A)-[read]->(B)\n(A)-<revoke>-x[ab]")?;
    let ineffective = parse("rain\nab: (A)-[read]->(B)\n(C)-<revoke>-x[ab]")?;

    let svg = effective.to_rain_svg();
    assert!(svg.contains(r#"<path class="cross""#), "{svg}");
    assert!(effective.to_text().contains('✗'));

    let svg = ineffective.to_rain_svg();
    let group = svg
        .find(r#"<g class="event revoke unauthorized""#)
        .and_then(|at| svg.get(at..at + svg[at..].find("</g>")?))
        .ok_or("the revocation's group")?;
    assert!(!group.contains(r#"class="cross""#), "{group}");
    assert!(
        group.contains("<title>No effect: C can't revoke ab</title>"),
        "{group}"
    );
    let text = ineffective.to_text();
    assert!(!text.contains('✗'), "{text}");
    Ok(())
}

/// Every number in the opening tag matching `needle`'s attribute `key`.
fn attr(tag: &str, key: &str) -> Option<f64> {
    let from = tag.find(&format!(r#" {key}=""#))? + key.len() + 3;
    tag.get(from..from + tag[from..].find('"')?)?.parse().ok()
}

/// The opening tags of every element whose tag contains `needle`.
fn tags_with<'a>(svg: &'a str, needle: &str) -> Vec<&'a str> {
    svg.match_indices(needle)
        .filter_map(|(at, _)| {
            let start = svg[..at].rfind('<')?;
            svg.get(start..=at + svg[at..].find('>')?)
        })
        .collect()
}

/// Exact handle positions, which generated scenarios rarely reach: beads
/// on a membership's arrow start at the box's audience-side edge and step
/// 12 px along it; a late route's handle sits on the member's lifeline, on
/// the boundary below the row it comes from.
#[test]
fn handles_sit_exactly_where_they_should() -> TestResult {
    let svg = parse(
        "rain
         (Doc)-[admin]->(Team)
         (Team)-[admin]->(Eve)
         (Eve)=[[edit]]=>(Doc)
         db: (Doc)-[read]->(Bob)
         (Eve)-<revoke>-x[db]",
    )?
    .to_rain_svg();
    // The membership's own group (a root circle elsewhere also says event 1)
    let pill = svg
        .find(r#"<g class="event delegate valid" data-event="1""#)
        .and_then(|at| tag_with(&svg[at..], r#"class="pill""#))
        .ok_or("membership box")?;
    let right = attr(pill, "x").ok_or("x")? + attr(pill, "width").ok_or("width")?;
    let row = attr(pill, "y").ok_or("y")? + 11.0;
    let mut beads: Vec<f64> = tags_with(&svg, r#"class="handle""#)
        .iter()
        .filter(|t| t.contains(r#"data-event="1""#))
        .map(|t| {
            assert!(
                (attr(t, "cy").unwrap_or_default() - row).abs() < 0.11,
                "{t}"
            );
            attr(t, "cx").unwrap_or_default() - right
        })
        .collect();
    beads.sort_by(f64::total_cmp);
    assert_eq!(beads.len(), 2, "{beads:?}");
    assert!(
        beads[0].abs() < 0.11 && (beads[1] - 12.0).abs() < 0.11,
        "{beads:?}"
    );
    // Each route's arc arrives on top of its bead, not inside or under it
    for bead in &beads {
        let x = right + bead;
        let arrives = tags_with(&svg, r#"data-to="1""#).iter().any(|t| {
            t.contains("class=\"powerline")
                && t.split(r#"d=""#)
                    .nth(1)
                    .and_then(|d| d.split('"').next())
                    .and_then(|d| d.rsplit(' ').next())
                    .and_then(|end| end.split_once(','))
                    .and_then(|(ex, ey)| Some((ex.parse::<f64>().ok()?, ey.parse::<f64>().ok()?)))
                    .is_some_and(|(ex, ey)| {
                        (ex - x).abs() < 0.11 && (ey - (row - 5.0)).abs() < 0.11
                    })
        });
        assert!(arrives, "no arc lands on top of the bead at {x}\n{svg}");
    }

    let late = parse(
        "rain
         zoe-alice: (Zoe)-[admin]->(Alice)
         (Doc)-[read]->(Zoe)
         (Alice)-[read(Doc)]->(Bob)",
    )?
    .to_rain_svg();
    let alice = tag_with(&late, r#"data-principal="alice""#)
        .and_then(|t| attr(t, "data-x"))
        .ok_or("Alice's column")?;
    let top = tag_with(&late, "data-rows-top")
        .and_then(|t| attr(t, "data-rows-top"))
        .ok_or("rows top")?;
    let handle = tag_with(&late, r#"class="handle""#).ok_or("a handle")?;
    assert!(
        (attr(handle, "cx").ok_or("cx")? - alice).abs() < 0.11,
        "{handle}"
    );
    // Below row 1 (Doc→Zoe), on the boundary with row 2
    assert!(
        (attr(handle, "cy").ok_or("cy")? - (top + 2.0 * 64.0)).abs() < 0.11,
        "{handle}"
    );
    Ok(())
}

/// A revocation slid along its line by hand: the line runs from the
/// revoker to the diamond's near side, carries on from its far side to the
/// target's column, then up to just under the target.
#[test]
fn placed_revocations_route_around_their_diamond() -> TestResult {
    // The target's label is placed near C, so a diamond a fifth of the way
    // from A is well clear of both
    let svg = parse("rain\nab: (A)------[read]->(C)\n(A)-<revoke>----x[ab]")?.to_rain_svg();
    let line = tag_with(&svg, r#"class="revocation""#).ok_or("line")?;
    let d = line
        .split(r#"d=""#)
        .nth(1)
        .and_then(|d| d.split('"').next())
        .ok_or("d")?;
    let numbers: Vec<f64> = d
        .split(|c: char| c.is_ascii_alphabetic() || c == ',' || c == ' ')
        .filter_map(|n| n.parse().ok())
        .collect();
    // M x,y H near M far,y H tx V up
    let [x, y, near, far, far_y, tx, up] = numbers[..] else {
        return Err(format!("unexpected path {d}").into());
    };
    let diamond = tag_with(&svg, r#"class="diamond""#).ok_or("diamond")?;
    let cx = diamond
        .split(r#"d="M"#)
        .nth(1)
        .and_then(|d| d.split(',').next())
        .and_then(|n| n.parse::<f64>().ok())
        .ok_or("diamond centre")?;
    assert!(x < near && (near - (cx - 46.0)).abs() < 0.11, "{d}");
    assert!(
        (far - (cx + 46.0)).abs() < 0.11 && (far_y - y).abs() < 0.11,
        "{d}"
    );
    // Then up the page to the target, which is on an earlier row
    assert!(tx > far && up < y, "{d}");
    // A fifth of the way along, clamped clear of the revoker, not under it
    assert!(cx < tx - 46.0, "{d}");

    // And mirrored: revoking from the right goes the other way round
    let svg = parse("rain\nprincipals: C, B, A\nab: (A)------[read]->(C)\n(A)-<revoke>----x[ab]")?
        .to_rain_svg();
    let line = tag_with(&svg, r#"class="revocation""#).ok_or("line")?;
    let d = line
        .split(r#"d=""#)
        .nth(1)
        .and_then(|d| d.split('"').next())
        .ok_or("d")?;
    let numbers: Vec<f64> = d
        .split(|c: char| c.is_ascii_alphabetic() || c == ',' || c == ' ')
        .filter_map(|n| n.parse().ok())
        .collect();
    let [x, _, near, far, _, tx, _] = numbers[..] else {
        return Err(format!("unexpected path {d}").into());
    };
    assert!(x > near && near > far && far > tx, "right to left: {d}");
    assert!(
        (near - far - 92.0).abs() < 0.11,
        "the diamond is 92 wide: {d}"
    );
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
