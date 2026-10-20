//! The default path on a real part: settled with no tolerancing asked for, Specify proposes no
//! geometric scheme; asked for GD&T, the automatic answers are given back and it proposes one again.

mod common;

use specify_core::api;
use specify_core::tolerancing::{self, Mode, State};

#[test]
fn a_part_settles_to_the_default_path_and_gdt_gives_its_answers_back() {
    let plate = common::fixtures().join("parts").join("plate.step");
    let analysis = api::analyse_file(&plate, None).unwrap();
    let fresh = tolerancing::opened(
        &State {
            auto: Some(vec![]),
            ..State::default()
        },
        None,
    );
    let (review, settled) = tolerancing::settle(&analysis, &fresh).unwrap();
    assert_eq!(settled.gdt, None);
    assert_eq!(settled.answers["part.locating"], "± dimensions");
    let auto = settled.auto.clone().unwrap();
    assert!(auto.contains(&"part.locating".to_string()), "{auto:?}");
    assert!(auto.iter().all(|id| settled.answers.contains_key(id)));
    let datums = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|q| q["id"].as_str().is_some_and(|id| id.starts_with("datum.")))
        .count();
    assert_eq!(
        datums, 3,
        "A/B/C measurement origins are asked on the default path"
    );
    let preview = api::preview(&analysis, &settled.answers).unwrap();
    assert!(
        preview.datums.is_empty(),
        "#20: measurement origins are suggestions until answered"
    );
    assert!(preview.requirements.iter().all(|r| r["kind"] != "position"));
    let mut accepted = settled.answers.clone();
    for q in api::questions(&analysis, &accepted).unwrap() {
        if ["datum.A", "datum.B", "datum.C"].contains(&q.id.as_str()) {
            accepted.insert(q.id, q.default.unwrap());
        }
    }
    assert_eq!(api::preview(&analysis, &accepted).unwrap().datums.len(), 3);
    assert!(
        review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|q| q["id"].as_str().is_some_and(|id| id.starts_with("datum.")))
            .all(|q| q["prompt"].as_str().unwrap().contains("measured from"))
    );

    let geometric =
        tolerancing::tolerancing_edit(&settled, Some(Mode::Geometric), &settled.answers);
    assert!(!geometric.answers.contains_key("part.locating"));
    let (review, again) = tolerancing::settle(&analysis, &geometric).unwrap();
    assert_eq!(again, geometric, "nothing is added once GD&T is asked for");
    assert!(
        review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q["id"].as_str().is_some_and(|id| id.starts_with("datum.")))
    );
}
