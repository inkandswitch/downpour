//! The worked examples from the Keyhive Lab Notes,
//! [#7](https://www.inkandswitch.com/keyhive/notebook/07/) (certflow) and
//! [#8](https://www.inkandswitch.com/keyhive/notebook/08/) (rain), written in
//! the textual notation.

use rain_diagram_core::{
    diagram::{Diagram, Event, Principal},
    parse,
};
use testresult::TestResult;

fn ids(d: &Diagram) -> Vec<&str> {
    d.events()
        .iter()
        .filter_map(|e| match &e.node {
            Event::Delegate(c) => Some(c.id.as_str()),
            Event::Invoke(_) | Event::Revoke(_) => None,
        })
        .collect()
}

/// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/), "Plain Text", with every subject left to inference.
#[test]
fn rain_plain_text_example() -> TestResult {
    let d = parse(
        "rain
         (Doc)-[edit]->(Alice)            : 1. The Doc grants Alice Edit
         (Alice)-[edit]->(Bob)            : 2. Alice passes Edit to Bob
         (Bob)=[[edit]]=>(Doc)            : 3. Bob edits the Doc
         (Bob)-[read]->(Carol)            : 4. Bob gives Carol Read
         carol-dan: (Carol)-[read]->(Dan) : 5. Carol shares Read with Dan
         (Alice)-<revoke>-x[carol-dan]    : 6. Alice revokes Carol→Dan",
    )?;

    assert_eq!(
        d.principals()
            .iter()
            .map(Principal::key)
            .collect::<Vec<_>>(),
        ["doc", "alice", "bob", "carol", "dan"]
    );
    assert_eq!(
        ids(&d),
        ["doc-alice", "alice-bob", "bob-carol", "carol-dan"]
    );

    let json = d.to_site_json()?;
    assert!(json.contains(
        r#"{"id":"bob-carol","from":"bob","to":"carol","level":"read","subject":"doc"}"#
    ));
    assert!(json.contains(r#"{"invoke":"edit","by":"bob","on":"doc"}"#));
    assert!(json.contains(r#"{"revoke":"carol-dan","by":"alice"}"#));
    Ok(())
}

/// [Keyhive Lab Note #7](https://www.inkandswitch.com/keyhive/notebook/07/), "A More Involved Example": an essay shared with commenters.
/// Alice holds both the Essay and Commenters, so subjects must be explicit.
#[test]
fn certflow_commenters_example() -> TestResult {
    let d = parse(
        "certflow
         title An essay with comment threads

         (Essay)-[admin]->(Alice)
         (Alice)-[read(Essay)]->(Commenters)
         (Commenters)-[admin]->(Alice)
         (Alice)-[edit(Commenters)]->(Bob)
         (Alice)-[edit(Commenters)]->(Carol)
         (Comment Thread 1)-[admin]->(Commenters)
         (Comment Thread 2)-[admin]->(Commenters)",
    )?;

    assert_eq!(
        ids(&d),
        [
            "essay-alice",
            "alice-commenters",
            "commenters-alice",
            "alice-bob",
            "alice-carol",
            "comment-thread-1-commenters",
            "comment-thread-2-commenters",
        ]
    );
    Ok(())
}

/// [Keyhive Lab Note #8](https://www.inkandswitch.com/keyhive/notebook/08/), "Transitive Authority": Alice reaches the Doc through Team.
#[test]
fn transitive_handle() -> TestResult {
    let d = parse(
        "rain
         (Doc)-[edit]->(Team)
         team-alice: (Team)-[admin]->(Alice)
         (Alice)-[edit(Doc) via team-alice]->(Bob)",
    )?;

    assert!(d.to_site_json()?.contains(r#""transitive":["team-alice"]"#));
    Ok(())
}
