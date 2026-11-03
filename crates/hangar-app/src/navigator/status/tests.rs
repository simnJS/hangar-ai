//! The sentences a row's counters say, and where a click on one goes.

use hangar_i18n::Locale;

use super::*;

const EN: Translator = Translator::new(Locale::En);
const FR: Translator = Translator::new(Locale::Fr);

fn pane(id: &str, name: &str) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        ..Pane::default()
    }
}

#[test]
fn names_read_as_a_list() {
    assert_eq!(list_names(EN, &[]), "");
    assert_eq!(list_names(EN, &["Ava"]), "Ava");
    assert_eq!(list_names(EN, &["Ava", "Ben"]), "Ava and Ben");
    assert_eq!(list_names(EN, &["Ava", "Ben", "Cleo"]), "Ava, Ben and Cleo");
    assert_eq!(list_names(FR, &["Ava", "Ben", "Cléo"]), "Ava, Ben et Cléo");
}

#[test]
fn one_agent_waiting_is_a_singular_sentence() {
    let eve = pane("p5", "Eve");
    assert_eq!(
        who_is(EN, AgentActivity::Waiting, &[&eve]),
        "Eve waits for your answer"
    );
    assert_eq!(
        who_is(FR, AgentActivity::Waiting, &[&eve]),
        "Eve attend ta réponse"
    );
}

#[test]
fn several_agents_working_are_a_plural_one() {
    let (eve, max) = (pane("p5", "Eve"), pane("p6", "Max"));
    assert_eq!(
        who_is(EN, AgentActivity::Working, &[&eve, &max]),
        "Eve and Max are working"
    );
    assert_eq!(
        who_is(FR, AgentActivity::Working, &[&eve, &max]),
        "Eve et Max travaillent"
    );
}

#[test]
fn done_agents_say_it_is_your_turn() {
    let eve = pane("p5", "Eve");
    assert_eq!(
        who_is(EN, AgentActivity::Yours, &[&eve]),
        "Eve finished, your turn"
    );
}

fn ids() -> Vec<String> {
    ["a", "b", "c"].iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_first_click_goes_to_the_first_agent() {
    let panes = ids();
    assert_eq!(go_target(&panes, |id| id, None), Some(&"a".to_string()));
}

#[test]
fn each_click_goes_to_the_next_and_wraps() {
    let panes = ids();
    let after = |current: &str| go_target(&panes, |id| id, Some(&current.to_string())).cloned();
    assert_eq!(after("a"), Some("b".to_string()));
    assert_eq!(after("b"), Some("c".to_string()));
    assert_eq!(after("c"), Some("a".to_string()));
}

#[test]
fn a_pane_in_front_that_is_not_among_them_starts_from_the_first() {
    let panes = ids();
    assert_eq!(
        go_target(&panes, |id| id, Some(&"elsewhere".to_string())),
        Some(&"a".to_string())
    );
}

#[test]
fn nobody_in_the_state_has_nowhere_to_go() {
    let none: Vec<String> = Vec::new();
    assert_eq!(go_target(&none, |id| id, None), None);
}
