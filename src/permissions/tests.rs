use crate::{
    commands::Command,
    config::{GroupRef, PermissionsConfig},
    permissions::{Denied, Group, Role, check, role},
};

fn config() -> PermissionsConfig {
    PermissionsConfig {
        admin_groups: vec![GroupRef::Name("Admin".into()), GroupRef::Id(6)],
        blocked_groups: vec![GroupRef::Name("No Bot".into()), GroupRef::Id(9)],
    }
}

fn named(id: u64, name: &'static str) -> Group<'static> {
    Group {
        id,
        name: Some(name),
    }
}

fn unnamed(id: u64) -> Group<'static> {
    Group { id, name: None }
}

#[test]
fn no_groups_is_a_user() {
    assert_eq!(role(&config(), &[]), Role::User);
    assert_eq!(role(&config(), &[named(7, "Normal")]), Role::User);
}

#[test]
fn admin_by_name_ignores_case() {
    assert_eq!(role(&config(), &[named(2, "admin")]), Role::Admin);
    assert_eq!(role(&config(), &[named(2, "ADMIN")]), Role::Admin);
}

#[test]
fn name_must_match_whole() {
    assert_eq!(role(&config(), &[named(2, "Admins")]), Role::User);
    assert_eq!(role(&config(), &[named(2, "Server Admin")]), Role::User);
}

#[test]
fn non_ascii_names_ignore_case() {
    let config = PermissionsConfig {
        admin_groups: vec![GroupRef::Name("Właściciel".into())],
        blocked_groups: Vec::new(),
    };
    assert_eq!(role(&config, &[named(3, "WŁAŚCICIEL")]), Role::Admin);
}

#[test]
fn ids_match_without_names() {
    assert_eq!(role(&config(), &[unnamed(6)]), Role::Admin);
    assert_eq!(role(&config(), &[unnamed(9)]), Role::Blocked);
}

#[test]
fn names_do_not_match_without_the_group_list() {
    assert_eq!(role(&config(), &[unnamed(2)]), Role::User);
}

#[test]
fn a_name_is_not_read_as_an_id() {
    let config = PermissionsConfig {
        admin_groups: vec![GroupRef::Name("6".into())],
        blocked_groups: Vec::new(),
    };
    assert_eq!(role(&config, &[named(6, "Normal")]), Role::User);
}

#[test]
fn blocked_by_name_or_id() {
    assert_eq!(role(&config(), &[named(11, "no bot")]), Role::Blocked);
    assert_eq!(
        role(&config(), &[named(7, "Normal"), unnamed(9)]),
        Role::Blocked
    );
}

#[test]
fn admin_wins_over_blocked() {
    assert_eq!(
        role(&config(), &[named(11, "No Bot"), named(2, "Admin")]),
        Role::Admin
    );
}

#[test]
fn empty_config_makes_everyone_a_user() {
    let config = PermissionsConfig {
        admin_groups: Vec::new(),
        blocked_groups: Vec::new(),
    };
    assert_eq!(role(&config, &[named(2, "Admin")]), Role::User);
}

fn parse(text: &str) -> Command {
    Command::parse(text).unwrap().unwrap()
}

const CHANNEL_COMMANDS: [&str; 10] = [
    "!song add abba",
    "!song pause",
    "!song resume",
    "!song next",
    "!song current",
    "!bot volume",
    "!bot volume 30",
    "!queue show",
    "!queue remove 1",
    "!queue clear",
];

const CONFIG_COMMANDS: [&str; 4] = [
    "!bot config list",
    "!bot config get audio.bitrate",
    "!bot config set audio.bitrate 96",
    "!bot config reload",
];

const ANYWHERE_COMMANDS: [&str; 3] = ["!help", "!bot ping", "!bot summon"];

#[test]
fn user_in_the_bot_channel_controls_it() {
    for text in CHANNEL_COMMANDS.iter().chain(&ANYWHERE_COMMANDS) {
        assert_eq!(check(&parse(text), Role::User, true), Ok(()), "{text}");
    }
}

#[test]
fn user_elsewhere_only_asks_for_help_or_summons() {
    for text in CHANNEL_COMMANDS {
        assert_eq!(
            check(&parse(text), Role::User, false),
            Err(Denied::OtherChannel),
            "{text}"
        );
    }
    for text in ANYWHERE_COMMANDS {
        assert_eq!(check(&parse(text), Role::User, false), Ok(()), "{text}");
    }
}

#[test]
fn config_is_for_admins_only() {
    for text in CONFIG_COMMANDS {
        for in_bot_channel in [true, false] {
            assert_eq!(
                check(&parse(text), Role::User, in_bot_channel),
                Err(Denied::AdminOnly),
                "{text}"
            );
        }
        assert_eq!(check(&parse(text), Role::Admin, false), Ok(()), "{text}");
    }
}

#[test]
fn admin_controls_from_anywhere() {
    for text in CHANNEL_COMMANDS.iter().chain(&ANYWHERE_COMMANDS) {
        assert_eq!(check(&parse(text), Role::Admin, false), Ok(()), "{text}");
    }
}

#[test]
fn blocked_is_refused_everything() {
    let all = CHANNEL_COMMANDS
        .iter()
        .chain(&ANYWHERE_COMMANDS)
        .chain(&CONFIG_COMMANDS);
    for text in all {
        for in_bot_channel in [true, false] {
            assert_eq!(
                check(&parse(text), Role::Blocked, in_bot_channel),
                Err(Denied::Blocked),
                "{text}"
            );
        }
    }
}
