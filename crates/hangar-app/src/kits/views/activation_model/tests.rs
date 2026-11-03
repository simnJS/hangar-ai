//! The cases of `kitActivation.test.ts` ("the activation view") and of
//! `KitsSettings.test.ts` that drew `KitActivationView` to markup, against the
//! model the view is drawn from: its sentences, tags and decisions, in English
//! and French.

use std::collections::BTreeMap;

use hangar_core::kits::commands::CommandInfo;
use hangar_core::kits::{CommandArg, Hook, Inspection, KitInfo, StepStatus};
use hangar_i18n::{Locale, Translator};

use super::*;
use crate::kits::activation::{RunGroupKind, RunItem, running_groups_of};
use crate::kits::testing::{
    COMMANDS, HOOKS, OTHER_COMMANDS, OTHER_HOOKS, command_kit, entry, folder, git, git_kit,
    inspection, kit, native_tab, tab,
};

/// What a test changes about the view it draws.
struct Case<'a> {
    kit: &'a KitInfo,
    shown: Option<Inspection>,
    entry: Option<WorkspaceKit>,
    everywhere: bool,
    groups: Vec<RunGroup>,
    steps: Option<Vec<StepStatus>>,
    can_run: bool,
    running: Option<&'a str>,
    ran: Option<StepRun>,
}

impl<'a> Case<'a> {
    fn new(kit: &'a KitInfo) -> Case<'a> {
        Case {
            kit,
            shown: Some(default_inspection()),
            entry: None,
            everywhere: false,
            groups: Vec::new(),
            steps: Some(Vec::new()),
            can_run: true,
            running: None,
            ran: None,
        }
    }

    fn shown(mut self, shown: Inspection) -> Self {
        self.shown = Some(shown);
        self
    }

    fn entry(mut self, id: &str, granted: &[&str]) -> Self {
        self.entry = Some(entry(id, granted));
        self
    }

    fn draw(self, locale: Locale) -> ActivationModel {
        let input = ActivationInput {
            kit: self.kit,
            inspection: self.shown.as_ref(),
            steps: self.steps.as_deref(),
            entry: self.entry.as_ref(),
            workspace_name: Some("Studio"),
            everywhere: self.everywhere,
            can_run: self.can_run,
            os: KitOs::Windows,
            running: self.running,
            ran: self.ran.as_ref(),
            error: None,
        };
        ActivationModel::build_with(Translator::new(locale), &input, self.groups)
    }

    fn en(self) -> ActivationModel {
        self.draw(Locale::En)
    }
}

fn hook(event: &str, matcher: &str, command: &str) -> Hook {
    Hook {
        event: event.into(),
        matcher: matcher.into(),
        command: command.into(),
    }
}

fn default_hooks() -> Vec<Hook> {
    vec![
        hook(
            "PostToolUse",
            "Write|Edit",
            "echo \"edited\" >> \"$HANGAR_KITS_DATA/hello-kit/log\"",
        ),
        hook("SessionStart", "", "node ${CLAUDE_PLUGIN_ROOT}/start.js"),
    ]
}

fn default_inspection() -> Inspection {
    Inspection {
        hooks: default_hooks(),
        hooks_permission: Some(HOOKS.into()),
        ..inspection(vec![tab(&["loopback:8765", "data.read"])])
    }
}

/// French typesetting puts no-break spaces before `;` and `:`; the strings are
/// written with plain ones.
fn plain(text: String) -> String {
    text.replace(['\u{a0}', '\u{202f}'], " ")
}

fn text_of(model: &ActivationModel) -> String {
    plain(model.text())
}

use crate::kits::setup::KitOs;
use crate::state::types::WorkspaceKit;

// -- hooks ---------------------------------------------------------------------

#[test]
fn shows_the_hooks_verbatim_and_says_they_run_by_themselves() {
    let git = git_kit(Some(HOOKS), true);
    let model = Case::new(&git).en();
    let out = model.text();
    assert!(
        out.contains("Claude Code runs these by itself, without asking, each time they match.")
    );
    assert!(out.contains("PostToolUse · Write|Edit"));
    assert!(out.contains("SessionStart"));
    // The text is the command, whole.
    assert!(out.contains("echo \"edited\" >> \"$HANGAR_KITS_DATA/hello-kit/log\""));
    assert!(out.contains("node ${CLAUDE_PLUGIN_ROOT}/start.js"));
    assert_eq!(model.commands_shown().len(), 2);
}

#[test]
fn says_it_in_french_with_accents() {
    let git = git_kit(Some(HOOKS), true);
    let out = text_of(&Case::new(&git).draw(Locale::Fr));
    assert!(out.contains(
        "Claude Code les lance de lui-même, sans demander, chaque fois qu'ils correspondent."
    ));
    assert!(out.contains("parler au serveur local sur le port 8765"));
}

#[test]
fn says_in_plain_words_what_a_local_server_s_permission_lets_the_tab_do() {
    let git = git_kit(Some(HOOKS), true);
    let out = Case::new(&git).en().text();
    assert!(out.contains(
        "talk to the local server on port 8765, and make it do whatever that server allows"
    ));
    assert!(out.contains("read the data this kit keeps in its own folder"));
}

#[test]
fn says_when_a_plugin_runs_nothing_by_itself() {
    let git = git_kit(None, true);
    let shown = Inspection {
        hooks: vec![],
        hooks_permission: None,
        ..default_inspection()
    };
    let out = Case::new(&git).shown(shown).en().text();
    assert!(out.contains("Nothing runs by itself."));
    assert!(!out.contains("runs these by itself"));
}

// -- files.write ------------------------------------------------------------------

const SECRETS: &str = "so it can also read hidden files such as .env, and the secrets they hold";

fn with_tabs(tabs: Vec<hangar_core::kits::Tab>) -> Inspection {
    Inspection {
        hooks: vec![],
        ..inspection(tabs)
    }
}

#[test]
fn says_in_plain_words_that_a_tab_can_change_the_project_s_files() {
    let plain_kit = kit("1.0.0", vec![]);
    let out = Case::new(&plain_kit)
        .shown(with_tabs(vec![tab(&["files.write"])]))
        .en()
        .text();
    assert!(out.contains("change text files in your project, hidden ones like .env included"));
    assert!(!out.contains(SECRETS));
}

#[test]
fn names_env_and_secrets_for_a_tab_that_holds_both_with_what_could_carry_them_off() {
    let plain_kit = kit("1.0.0", vec![]);
    let both = with_tabs(vec![tab(&["files.read", "files.write"])]);
    let en = Case::new(&plain_kit).shown(both.clone()).en().text();
    assert!(en.contains(SECRETS));
    assert!(en.contains("panes.send"));
    let fr = text_of(&Case::new(&plain_kit).shown(both).draw(Locale::Fr));
    assert!(fr.contains("les fichiers cachés comme .env, et les secrets qu'ils contiennent"));
    assert!(fr.contains("panes.send"));
}

#[test]
fn says_nothing_of_secrets_for_a_tab_that_only_reads_or_across_two_tabs() {
    let plain_kit = kit("1.0.0", vec![]);
    let reads = Case::new(&plain_kit)
        .shown(with_tabs(vec![tab(&["files.read"])]))
        .en()
        .text();
    assert!(!reads.contains(SECRETS));
    let mut other = tab(&["files.write"]);
    other.id = "other".into();
    let apart = Case::new(&plain_kit)
        .shown(with_tabs(vec![tab(&["files.read"]), other]))
        .en()
        .text();
    assert!(!apart.contains(SECRETS));
}

#[test]
fn shows_what_an_update_added_as_new_and_what_asks_for_it_to_be_accepted() {
    let updated = kit("1.1.0", vec![tab(&["files.read", "files.write"])]);
    let model = Case::new(&updated)
        .shown(with_tabs(vec![tab(&["files.read", "files.write"])]))
        .entry("hello-kit", &["files.read"])
        .en();
    assert!(model.new_tags() > 0);
    assert!(model.text().contains(SECRETS));
    assert!(matches!(
        model.footer,
        Footer::Decide {
            accept_enabled: true,
            ..
        }
    ));
}

// -- {kitDir} ----------------------------------------------------------------------

fn command(name: &str, argv: &[&str], args: &[(&str, &str, Option<&str>)]) -> CommandInfo {
    CommandInfo {
        name: name.into(),
        description: "Runs the kit's audit.".into(),
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        args: args
            .iter()
            .map(|(name, pattern, description)| {
                (
                    (*name).to_owned(),
                    CommandArg {
                        pattern: (*pattern).into(),
                        description: description.map(str::to_owned),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        timeout_seconds: 30,
    }
}

fn with_commands(commands: Vec<CommandInfo>) -> Inspection {
    Inspection {
        hooks: vec![],
        declared_commands: commands,
        ..default_inspection()
    }
}

#[test]
fn kit_dir_is_shown_as_written_with_no_rule_of_its_own_and_said_to_be_the_kit_s_folder() {
    let plain_kit = kit("1.0.0", vec![]);
    let audit = command(
        "audit",
        &["node", "{kitDir}/tools/audit.js", "{id}"],
        &[("id", "[0-9]+", None)],
    );
    let model = Case::new(&plain_kit)
        .shown(with_commands(vec![audit.clone()]))
        .en();
    let out = model.text();
    assert!(out.contains("node {kitDir}/tools/audit.js {id}"));
    let Loadable::Ready(body) = &model.commands.body else {
        panic!("commands are ready");
    };
    let placeholders: Vec<_> = body.items[0]
        .rules
        .iter()
        .map(|r| r.placeholder.as_str())
        .collect();
    assert_eq!(placeholders, ["{id}"]);
    assert!(out.contains("{kitDir} is this kit's own folder"));
    let fr = text_of(
        &Case::new(&plain_kit)
            .shown(with_commands(vec![audit]))
            .draw(Locale::Fr),
    );
    assert!(fr.contains("{kitDir} est le dossier de ce kit"));
}

#[test]
fn kit_dir_is_not_spoken_of_for_a_command_that_does_not_use_it() {
    let plain_kit = kit("1.0.0", vec![]);
    let gh = command(
        "audit",
        &["gh", "run", "view", "{id}"],
        &[("id", "[0-9]+", None)],
    );
    let out = Case::new(&plain_kit)
        .shown(with_commands(vec![gh]))
        .en()
        .text();
    assert!(!out.contains("{kitDir}"));
}

// -- what else a plugin starts by itself -----------------------------------------------

fn running() -> Vec<(RunGroupKind, RunItem)> {
    let item = |name: &str| RunItem {
        name: name.into(),
        ..RunItem::default()
    };
    vec![
        (
            RunGroupKind::Mcp,
            RunItem {
                argv: vec![
                    "node".into(),
                    "${CLAUDE_PLUGIN_ROOT}/server.js".into(),
                    "4 4".into(),
                ],
                env: vec!["API_TOKEN".into(), "ROOT".into()],
                ..item("local")
            },
        ),
        (
            RunGroupKind::Mcp,
            RunItem {
                text: Some("https://api.example.com/mcp".into()),
                headers: vec!["Authorization".into()],
                ..item("remote")
            },
        ),
        (
            RunGroupKind::Mcp,
            RunItem {
                text: Some("node ./headers.js".into()),
                ..item("remote · headersHelper")
            },
        ),
        (
            RunGroupKind::Mcp,
            RunItem {
                text: Some("./packed.mcpb".into()),
                unreadable: true,
                ..item("./packed.mcpb")
            },
        ),
        (
            RunGroupKind::Monitor,
            RunItem {
                text: Some("./poll.sh".into()),
                when: Some("on-skill-invoke:deploy".into()),
                description: Some("Deploys".into()),
                ..item("deploy")
            },
        ),
        (
            RunGroupKind::Lsp,
            RunItem {
                argv: vec!["gopls".into(), "serve".into()],
                when: Some(".go, .mod".into()),
                ..item("gopls")
            },
        ),
        (
            RunGroupKind::SkillHook,
            RunItem {
                text: Some("hooks:\n  Stop: check.sh".into()),
                ..item("skills/guarded/SKILL.md")
            },
        ),
        (
            RunGroupKind::Settings,
            RunItem {
                text: Some("./line.sh".into()),
                ..item("subagentStatusLine")
            },
        ),
        (
            RunGroupKind::Grammar,
            RunItem {
                text: Some("npm:hljs-gdscript@1.0.0".into()),
                unreadable: true,
                ..item("gdscript")
            },
        ),
        (
            RunGroupKind::Bin,
            RunItem {
                text: Some("bin/tool".into()),
                ..item("tool")
            },
        ),
        (
            RunGroupKind::Binary,
            RunItem {
                text: Some("sha256:abcd".into()),
                unreadable: true,
                ..item("fetched")
            },
        ),
        (
            RunGroupKind::Dependency,
            RunItem {
                text: Some("audit-logger".into()),
                unreadable: true,
                ..item("audit-logger")
            },
        ),
        (
            RunGroupKind::Unknown,
            RunItem {
                text: Some("node ./boot.js".into()),
                ..item("plugin.json › startup")
            },
        ),
        (
            RunGroupKind::Unknown,
            RunItem {
                unreadable: true,
                ..item(".lsp.json")
            },
        ),
    ]
}

fn shown_with_running() -> (Inspection, Vec<RunGroup>) {
    let mut hooks = default_hooks();
    hooks.push(hook("modules", "", "./register.js"));
    let groups = running_groups_of(&hooks, running());
    (
        Inspection {
            hooks,
            ..default_inspection()
        },
        groups,
    )
}

fn with_running(case: Case<'_>) -> Case<'_> {
    let (shown, groups) = shown_with_running();
    Case {
        shown: Some(shown),
        groups,
        ..case
    }
}

#[test]
fn shows_each_one_as_written() {
    let git = git_kit(Some(HOOKS), true);
    let out = with_running(Case::new(&git)).en().text();
    for line in [
        "node ${CLAUDE_PLUGIN_ROOT}/server.js \"4 4\"",
        "https://api.example.com/mcp",
        "node ./headers.js",
        "./poll.sh",
        "deploy · on-skill-invoke:deploy",
        "Deploys",
        "gopls serve",
        "gopls · .go, .mod",
        "hooks:\n  Stop: check.sh",
        "./line.sh",
        "bin/tool",
        "./register.js",
    ] {
        assert!(out.contains(line), "{line}");
    }
}

#[test]
fn gives_only_the_names_of_what_it_sets_and_sends() {
    let git = git_kit(Some(HOOKS), true);
    let out = with_running(Case::new(&git)).en().text();
    assert!(out.contains("Environment variables it sets (values not shown): API_TOKEN, ROOT"));
    assert!(out.contains("Headers it sends (values not shown): Authorization"));
}

#[test]
fn says_plainly_when_each_kind_runs() {
    let git = git_kit(Some(HOOKS), true);
    let out = with_running(Case::new(&git)).en().text();
    for (title, sentence) in [
        (
            "Hooks modules",
            "Claude Code loads this code into itself when a session starts",
        ),
        (
            "MCP servers",
            "when a session starts, and keeps it for the whole session",
        ),
        (
            "Monitors",
            "when a session starts, or the first time the skill it names runs",
        ),
        (
            "Language servers",
            "the first time Claude edits a file of the types shown",
        ),
        (
            "Hooks of skills and commands",
            "While that skill or command is in use",
        ),
        ("Settings", "They apply when a session starts"),
        ("Highlighting grammars", "Claude Code loads this JavaScript"),
        (
            "Programs Claude can call by name",
            "Nothing starts these by itself",
        ),
        ("Programs to download", "Claude Code may download these"),
        (
            "Other plugins",
            "Claude Code enables these along with this one",
        ),
        ("Not understood", "Hangar does not know what these are"),
    ] {
        assert!(out.contains(title), "{title}");
        assert!(out.contains(sentence), "{sentence}");
    }
}

#[test]
fn says_what_hangar_cannot_read_and_the_sentence_of_each_kind_in_french_with_accents() {
    let git = git_kit(Some(HOOKS), true);
    let out = text_of(&with_running(Case::new(&git)).draw(Locale::Fr));
    assert!(
        out.contains("Ne peut pas être lu par Hangar : ce que cela lance n'est pas montré ici.")
    );
    assert!(out.contains("Non compris"));
    assert!(out.contains("Moniteurs"));
    assert!(out.contains("la première fois que le skill qu'il nomme se lance"));
    assert!(
        out.contains(
            "Variables d'environnement qu'il définit (valeurs masquées) : API_TOKEN, ROOT"
        )
    );
}

#[test]
fn says_it_of_each_bundle_remote_grammar_download_other_plugin_and_broken_file() {
    let git = git_kit(Some(HOOKS), true);
    let out = with_running(Case::new(&git)).en().text();
    let unreadable = running().iter().filter(|(_, item)| item.unreadable).count();
    assert_eq!(out.matches("Cannot be read by Hangar").count(), unreadable);
    // A broken file has nothing to show but its name, and its name is shown.
    let model = with_running(Case::new(&git)).en();
    let Loadable::Ready(body) = &model.runs.body else {
        panic!("ready");
    };
    let broken = body
        .groups
        .iter()
        .flat_map(|group| &group.items)
        .find(|item| item.title == ".lsp.json")
        .expect("the broken file is listed");
    assert_eq!(broken.line, None);
}

#[test]
fn runs_nothing_is_not_said_when_only_the_rest_runs() {
    let git = git_kit(Some(HOOKS), true);
    let groups = running_groups_of(&[], running().into_iter().take(1));
    let shown = Inspection {
        hooks: vec![],
        ..default_inspection()
    };
    let case = Case {
        shown: Some(shown),
        groups,
        ..Case::new(&git)
    };
    let out = case.en().text();
    assert!(!out.contains("Nothing runs by itself."));
    assert!(out.contains("MCP servers"));
    // No plain hooks were listed.
    assert!(!out.contains("Claude Code runs these by itself, without asking"));
}

#[test]
fn asks_once_for_a_kit_accepted_when_the_list_held_only_its_hooks_and_marks_all_of_it() {
    let git = git_kit(Some(OTHER_HOOKS), true);
    let (shown, groups) = shown_with_running();
    let shown = Inspection {
        hooks_permission: Some(OTHER_HOOKS.into()),
        ..shown
    };
    let model = Case {
        shown: Some(shown),
        groups,
        ..Case::new(&git)
    }
    .entry("hello-kit", &["loopback:8765", "data.read", HOOKS])
    .en();
    let out = model.text();
    assert!(out.contains("was not shown in full then"));
    assert!(out.contains("this kit stays out of the workspace's new panes"));
    assert!(out.contains("Accept what changed"));
    // Every hook, every module and everything else it starts.
    let steps = default_hooks().len() + running().len() + 1;
    assert_eq!(model.new_tags(), steps);
}

#[test]
fn does_not_ask_once_accepted_and_accepting_stores_what_was_shown() {
    let git = git_kit(Some(HOOKS), true);
    let model = with_running(Case::new(&git))
        .entry("hello-kit", &["panes.read", HOOKS])
        .en();
    let out = model.text();
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(!out.contains("was not shown in full then"));
    let shown = Inspection {
        hooks_permission: Some(HOOKS.into()),
        ..inspection(vec![])
    };
    let changed = git_kit(Some(OTHER_HOOKS), true);
    assert_eq!(
        crate::kits::activation::accepted_grants(&changed.source, Some(&shown)),
        vec![HOOKS.to_owned()]
    );
}

#[test]
fn does_not_ask_about_a_linked_folder_though_it_shows_the_list() {
    let linked = KitInfo {
        source: folder(),
        ..git_kit(Some(OTHER_HOOKS), true)
    };
    let model = with_running(Case::new(&linked))
        .entry("hello-kit", &["panes.read"])
        .en();
    let out = model.text();
    assert!(out.contains("MCP servers"));
    assert!(out.contains("A linked folder is read live"));
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(!out.contains("stays out"));
}

#[test]
fn marks_hooks_that_changed_and_asks_to_accept_them() {
    let git = git_kit(Some(OTHER_HOOKS), true);
    let model = Case::new(&git)
        .entry(
            "hello-kit",
            &["loopback:8765", "data.read", "panes.read", HOOKS],
        )
        .en();
    let out = model.text();
    assert!(out.contains("this kit stays out of the workspace's new panes"));
    assert!(out.contains("Accept what changed"));
    assert!(model.new_tags() > 0);
}

#[test]
fn closes_without_asking_once_these_very_hooks_are_accepted() {
    let git = git_kit(Some(HOOKS), true);
    let model = Case::new(&git)
        .entry("hello-kit", &["panes.read", HOOKS])
        .en();
    let out = model.text();
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(out.contains("Already accepted"));
    assert!(!out.contains("stays out"));
}

#[test]
fn does_not_ask_about_a_linked_folder_s_hooks_and_says_they_apply_as_they_are_edited() {
    let linked = KitInfo {
        source: folder(),
        ..git_kit(Some(OTHER_HOOKS), true)
    };
    let model = Case::new(&linked).entry("hello-kit", &["panes.read"]).en();
    let out = model.text();
    assert!(out.contains("A linked folder is read live"));
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(!out.contains("Accept what changed"));
    assert!(!out.contains("stays out"));
}

// -- a kit's commands -------------------------------------------------------------------

fn ci_run() -> CommandInfo {
    CommandInfo {
        description: "The state of a CI run.".into(),
        ..command(
            "ci-run",
            &["gh", "run", "view", "{run}", "--repo={repo}"],
            &[
                ("repo", "[\\w.-]+/[\\w.-]+", None),
                ("run", "[0-9]+", Some("The run's number")),
            ],
        )
    }
}

fn shown_commands() -> Inspection {
    Inspection {
        hooks: vec![],
        hooks_permission: None,
        declared_commands: vec![ci_run()],
        commands_permission: Some(COMMANDS.into()),
        ..inspection(vec![tab(&["command:ci-run"])])
    }
}

fn github() -> hangar_core::kits::Source {
    git("https://github.com/o/kits", None)
}

#[test]
fn shows_each_command_verbatim_with_its_sentence_and_each_rule() {
    let kit = command_kit(Some(COMMANDS), github());
    let model = Case::new(&kit).shown(shown_commands()).en();
    let out = model.text();
    assert!(out.contains("Commands its tabs can run"));
    assert!(out.contains("The state of a CI run."));
    assert!(out.contains("gh run view {run} --repo={repo}"));
    assert!(out.contains(
        "A tab of this kit can run this command on your computer whenever it wants, with your logins, for instance your gh login; only the parts in braces change, and they must match their rule."
    ));
    // The rule sits with its placeholder, and they stay as written.
    assert!(out.contains("{run} [0-9]+ The run's number"));
    assert!(out.contains("{repo} [\\w.-]+/[\\w.-]+"));
}

#[test]
fn says_the_commands_in_french_with_accents() {
    let kit = command_kit(Some(COMMANDS), github());
    let out = text_of(&Case::new(&kit).shown(shown_commands()).draw(Locale::Fr));
    assert!(out.contains("Commandes que ses onglets peuvent lancer"));
    assert!(out.contains(
        "Un onglet de ce kit peut lancer cette commande sur votre ordinateur quand il le veut, avec vos identifiants, par exemple votre connexion gh ; seules les parties entre accolades changent, et elles doivent respecter leur règle."
    ));
    assert!(out.contains("lancer la commande ci-run sur votre ordinateur, avec vos identifiants"));
}

#[test]
fn does_not_speak_of_parts_that_change_when_nothing_does() {
    let kit = command_kit(Some(COMMANDS), github());
    let fixed = command("ci-run", &["gh", "run", "list"], &[]);
    let model = Case::new(&kit).shown(with_commands(vec![fixed])).en();
    let out = model.text();
    assert!(out.contains(
        "whenever it wants, with your logins, for instance your gh login; nothing in it changes."
    ));
    assert!(!out.contains("parts in braces"));
    let Loadable::Ready(body) = &model.commands.body else {
        panic!("ready");
    };
    assert!(body.items[0].rules.is_empty());
}

#[test]
fn says_when_its_tabs_run_nothing() {
    let kit = command_kit(None, github());
    let shown = Inspection {
        declared_commands: vec![],
        commands_permission: None,
        ..shown_commands()
    };
    let out = Case::new(&kit).shown(shown).en().text();
    assert!(out.contains("No command: its tabs run nothing on your computer."));
    assert!(!out.contains("whenever it wants"));
}

#[test]
fn counts_the_commands_among_what_a_kit_brings() {
    let kit = command_kit(Some(COMMANDS), github());
    let shown = Inspection {
        tabs: vec![],
        ..shown_commands()
    };
    let out = Case::new(&kit).shown(shown).en().text();
    assert!(!out.contains("Nothing for Claude Code and no tab."));
}

#[test]
fn marks_commands_that_changed_and_asks_to_accept_them() {
    let kit = command_kit(Some(OTHER_COMMANDS), github());
    let model = Case::new(&kit)
        .shown(shown_commands())
        .entry("hello-kit", &["command:ci-run", COMMANDS])
        .en();
    let out = model.text();
    assert!(out.contains("Until you accept them, this kit's tabs run none of them."));
    assert!(out.contains("Accept what changed"));
    assert!(model.new_tags() > 0);
}

#[test]
fn closes_without_asking_once_these_very_commands_are_accepted() {
    let kit = command_kit(Some(COMMANDS), github());
    let model = Case::new(&kit)
        .shown(shown_commands())
        .entry("hello-kit", &["command:ci-run", COMMANDS])
        .en();
    let out = model.text();
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(out.contains("Already accepted"));
    assert!(!out.contains("run none of them"));
}

#[test]
fn does_not_ask_about_a_linked_folder_s_commands_and_says_they_apply_as_they_are_edited() {
    let kit = command_kit(Some(OTHER_COMMANDS), folder());
    let model = Case::new(&kit)
        .shown(shown_commands())
        .entry("hello-kit", &["command:ci-run"])
        .en();
    let out = model.text();
    assert!(
        out.contains("A linked folder is read live: what you change in these commands applies")
    );
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(!out.contains("Accept what changed"));
}

// -- on all workspaces -------------------------------------------------------------------

#[test]
fn does_not_name_a_workspace_when_accepting_holds_for_all_of_them() {
    let git = git_kit(Some(HOOKS), true);
    let every = Case {
        everywhere: true,
        ..Case::new(&git)
    };
    let out = every.en().text();
    assert!(out.contains("On all workspaces"));
    assert!(!out.contains("Studio"));
    assert!(Case::new(&git).en().text().contains("Studio"));
    let fr = Case {
        everywhere: true,
        ..Case::new(&git)
    }
    .draw(Locale::Fr)
    .text();
    assert!(fr.contains("Sur tous les workspaces"));
}

// -- KitsSettings.test.ts: the activation view -----------------------------------------------

fn roblox() -> KitInfo {
    let mut found = KitInfo {
        id: "roblox".into(),
        source: git("https://github.com/o/kits", Some("main")),
        ..kit("1.0.0", vec![])
    };
    let manifest = found.manifest.as_mut().expect("manifest");
    manifest.id = "roblox".into();
    manifest.name = "Roblox".into();
    manifest.tabs = vec![tab(&["panes.send", "notify"])];
    found
}

fn roblox_inspection() -> Inspection {
    Inspection {
        skills: vec!["build".into()],
        commands: vec!["place".into()],
        mcp_servers: vec!["studio".into()],
        ..inspection(vec![tab(&["panes.send", "notify"])])
    }
}

fn step(done: bool) -> StepStatus {
    StepStatus {
        id: "deps".into(),
        title: "Install the server's dependencies".into(),
        description: None,
        command: "npm install --prefix server".into(),
        sha256: "x".into(),
        done,
    }
}

fn setup_case(kit: &KitInfo) -> Case<'_> {
    Case {
        shown: Some(roblox_inspection()),
        steps: Some(vec![step(false)]),
        ..Case::new(kit)
    }
}

#[test]
fn lists_what_the_kit_brings_its_permissions_in_plain_words_and_the_exact_command() {
    let kit = roblox();
    let model = setup_case(&kit).en();
    let out = model.text();
    assert!(out.contains("build"));
    assert!(out.contains("studio"));
    assert!(out.contains("send messages to your agents"));
    assert!(out.contains("npm install --prefix server"));
    assert!(out.contains("Kits reach Claude Code panes only"));
    assert!(out.contains("Accept and enable"));
    assert_eq!(model.commands_shown(), ["npm install --prefix server"]);
}

#[test]
fn says_the_permissions_in_french() {
    let kit = roblox();
    assert!(
        setup_case(&kit)
            .draw(Locale::Fr)
            .text()
            .contains("envoyer des messages à vos agents")
    );
}

#[test]
fn marks_the_permissions_an_update_added_and_asks_to_accept_them_again() {
    let kit = roblox();
    let model = setup_case(&kit).entry("roblox", &["notify"]).en();
    assert!(model.new_tags() > 0);
    assert!(model.text().contains("Accept the permissions"));
}

fn run_of(model: &ActivationModel) -> (&StepAction, &Tag) {
    let Loadable::Ready((_, steps)) = &model.setup.body else {
        panic!("steps are ready");
    };
    (&steps[0].action, &steps[0].tag)
}

#[test]
fn offers_run_only_for_a_step_not_done_and_disables_it_before_the_workspace_exists() {
    let kit = roblox();
    let model = setup_case(&kit).en();
    assert!(
        matches!(run_of(&model).0, StepAction::Run { label, enabled: true, .. } if label == "Run")
    );

    let done = Case {
        steps: Some(vec![step(true)]),
        ..setup_case(&kit)
    }
    .en();
    assert_eq!(run_of(&done).0, &StepAction::None);
    assert_eq!(run_of(&done).1.tone, TagTone::Ok);

    let early = Case {
        can_run: false,
        ..setup_case(&kit)
    }
    .en();
    assert!(matches!(
        run_of(&early).0,
        StepAction::Run {
            enabled: false,
            needs_workspace: Some(_),
            ..
        }
    ));
}

#[test]
fn disables_run_while_a_step_opens_its_pane_or_awaits_its_answer() {
    let kit = roblox();
    let opening = Case {
        running: Some("deps"),
        ..setup_case(&kit)
    }
    .en();
    assert!(matches!(
        run_of(&opening).0,
        StepAction::Run { label, enabled: false, .. } if label == "Opening the pane…"
    ));
}

#[test]
fn asks_whether_it_worked_after_the_command_was_typed_never_marking_it_itself() {
    let kit = roblox();
    let model = Case {
        ran: Some(StepRun {
            step: "deps".into(),
            pane_name: "Ava".into(),
        }),
        ..setup_case(&kit)
    }
    .en();
    let out = model.text();
    assert!(out.contains("It worked, mark it done"));
    assert!(out.contains("Ava"));
    assert!(matches!(run_of(&model).0, StepAction::Ask { .. }));
}

#[test]
fn closes_without_accepting_once_everything_is_accepted() {
    let kit = roblox();
    let model = setup_case(&kit)
        .entry("roblox", &["panes.send", "notify"])
        .en();
    assert_eq!(model.footer, Footer::Close("Close".into()));
    assert!(!model.text().contains("Accept and enable"));
}

#[test]
fn waits_to_accept_until_the_inspection_has_loaded_and_the_kit_is_usable() {
    let kit = roblox();
    let loading = Case {
        shown: None,
        ..Case::new(&kit)
    }
    .en();
    assert!(matches!(
        loading.footer,
        Footer::Decide {
            accept_enabled: false,
            ..
        }
    ));
    let mut broken = roblox();
    broken.error = Some("needs a newer Hangar".into());
    let model = setup_case(&broken).en();
    assert!(matches!(
        model.footer,
        Footer::Decide {
            accept_enabled: false,
            ..
        }
    ));
}

#[test]
fn titles_the_dialog_for_a_new_kit_or_a_review() {
    let kit = roblox();
    assert_eq!(setup_case(&kit).en().title, "Enable Roblox");
    assert_eq!(
        setup_case(&kit).entry("roblox", &["notify"]).en().title,
        "Roblox: review"
    );
}

// -- how a tab is made ---------------------------------------------------------------------

#[test]
fn says_how_each_tab_is_made_and_explains_a_native_one_once() {
    let kit = roblox();
    let mut html = tab(&["notify"]);
    html.title = "Page".into();
    let mut native = native_tab(&["panes.read"]);
    native.id = "native".into();
    native.title = "Native one".into();
    let mut both = tab(&["notify"]);
    both.id = "both".into();
    both.title = "Both".into();
    both.entry = "tabs/both.html".into();
    both.native = Some("tabs/both.wasm".into());
    let model = Case::new(&kit)
        .shown(with_tabs(vec![html, native, both]))
        .en();

    let Loadable::Ready(groups) = &model.brings.body else {
        panic!("ready");
    };
    let kinds: Vec<_> = groups[0]
        .chips
        .iter()
        .map(|chip| chip.kind.clone().expect("a tab has a kind"))
        .collect();
    assert_eq!(
        kinds,
        ["HTML", "Native", "Native, with an HTML page as a fallback"]
    );
    let out = model.text();
    assert!(out.contains("A native tab is a WebAssembly module"));
    // And each tab's permissions say it too.
    let Loadable::Ready(tabs) = &model.perms.body else {
        panic!("ready");
    };
    assert_eq!(tabs[1].kind, "Native");
}

#[test]
fn does_not_explain_native_tabs_when_there_are_none() {
    let kit = roblox();
    let model = setup_case(&kit).en();
    assert_eq!(model.brings.tab_kind_hint, None);
}

#[test]
fn says_the_tab_kinds_in_french() {
    let kit = roblox();
    let model = Case::new(&kit)
        .shown(with_tabs(vec![native_tab(&["notify"])]))
        .draw(Locale::Fr);
    let Loadable::Ready(groups) = &model.brings.body else {
        panic!("ready");
    };
    assert_eq!(groups[0].chips[0].kind.as_deref(), Some("Natif"));
    assert!(plain(model.text()).contains("Un onglet natif est un module WebAssembly"));
}
