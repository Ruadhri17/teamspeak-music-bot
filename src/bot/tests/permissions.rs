use super::harness::{Harness, author};
use crate::{permissions::Role, teamspeak::ReplyTo};

fn reply(text: &str) -> Option<String> {
    Some(text.to_string())
}

const PRIVATE: ReplyTo = ReplyTo::Client(7);

#[tokio::test]
async fn user_from_another_channel_is_refused() {
    let mut h = Harness::new();
    let outsider = author(7, Role::User, false);
    assert_eq!(
        h.send_as(PRIVATE, &outsider, "!song add a.mp3"),
        reply("join my channel to control me, or summon me with !bot summon")
    );
    assert_eq!(h.send("!queue show"), reply("the queue is empty"));
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn user_from_another_channel_may_summon() {
    let mut h = Harness::new();
    let outsider = author(7, Role::User, false);
    assert_eq!(h.send_as(PRIVATE, &outsider, "!bot summon"), None);
    assert_eq!(h.teamspeak_requests().len(), 1);
}

#[tokio::test]
async fn blocked_user_is_refused_even_in_the_channel() {
    let mut h = Harness::new();
    let blocked = author(7, Role::Blocked, true);
    for text in ["!song next", "!bot summon", "!help"] {
        assert_eq!(
            h.send_as(ReplyTo::Channel, &blocked, text),
            reply("you are not allowed to use this bot"),
            "{text}"
        );
    }
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn admin_controls_from_another_channel() {
    let mut h = Harness::new();
    let admin = author(7, Role::Admin, false);
    assert_eq!(h.send_as(PRIVATE, &admin, "!bot volume 30"), None);
    assert_eq!(h.send("!bot volume"), reply("volume: 30"));
}

#[tokio::test]
async fn config_is_refused_to_users() {
    let mut h = Harness::new();
    assert_eq!(
        h.send("!bot config list"),
        reply("only admins can use this command")
    );
}

#[tokio::test]
async fn usage_errors_come_before_permissions() {
    let mut h = Harness::new();
    let outsider = author(7, Role::User, false);
    assert!(h.send_as(PRIVATE, &outsider, "!song").is_some());
    assert_ne!(
        h.send_as(PRIVATE, &outsider, "!queue remove x"),
        reply("join my channel to control me, or summon me with !bot summon")
    );
}
