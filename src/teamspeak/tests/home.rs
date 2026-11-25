use crate::{
    config::ChannelRef,
    teamspeak::home::{ChannelInfo, Summon, resolve, summon},
};

fn channel(id: u64, parent: u64, name: &'static str) -> ChannelInfo<'static> {
    ChannelInfo {
        id,
        parent,
        name,
        is_default: false,
    }
}

fn tree() -> Vec<ChannelInfo<'static>> {
    vec![
        ChannelInfo {
            is_default: true,
            ..channel(1, 0, "Lobby")
        },
        channel(2, 0, "Music"),
        channel(3, 2, "Room"),
        channel(4, 0, "Games"),
        channel(5, 4, "Room"),
    ]
}

fn name(path: &str) -> Option<ChannelRef> {
    Some(ChannelRef::Name(path.to_owned()))
}

#[test]
fn no_home_channel_is_the_default_channel() {
    assert_eq!(resolve(None, &tree()), Some(1));
}

#[test]
fn home_channel_by_id_must_exist() {
    assert_eq!(resolve(Some(&ChannelRef::Id(3)), &tree()), Some(3));
    assert_eq!(resolve(Some(&ChannelRef::Id(99)), &tree()), None);
}

#[test]
fn home_channel_by_path_follows_the_parents() {
    assert_eq!(resolve(name("Music").as_ref(), &tree()), Some(2));
    assert_eq!(resolve(name("Music/Room").as_ref(), &tree()), Some(3));
    assert_eq!(resolve(name("Games/Room").as_ref(), &tree()), Some(5));
}

#[test]
fn home_channel_by_path_must_match_exactly() {
    for path in [
        "Room",
        "music",
        "Music/Nope",
        "Music/Room/Deeper",
        "",
        "Music/",
    ] {
        assert_eq!(resolve(name(path).as_ref(), &tree()), None, "{path:?}");
    }
}

#[test]
fn summon_in_the_same_channel_does_nothing() {
    assert_eq!(summon(2, 1, 2, false), Summon::Stay);
    assert_eq!(summon(1, 1, 1, false), Summon::Stay);
}

#[test]
fn summon_moves_the_bot_from_home() {
    assert_eq!(summon(1, 1, 3, false), Summon::Move(3));
}

#[test]
fn summon_is_refused_while_the_bot_is_elsewhere() {
    assert_eq!(summon(2, 1, 3, false), Summon::Busy);
    assert_eq!(summon(2, 1, 1, false), Summon::Busy);
}

#[test]
fn admin_takes_the_bot_from_any_channel() {
    assert_eq!(summon(2, 1, 3, true), Summon::Move(3));
    assert_eq!(summon(2, 1, 1, true), Summon::Move(1));
    assert_eq!(summon(1, 1, 3, true), Summon::Move(3));
}

#[test]
fn admin_in_the_bot_channel_does_nothing() {
    assert_eq!(summon(2, 1, 2, true), Summon::Stay);
}
