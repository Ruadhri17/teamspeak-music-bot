use std::time::Duration;

use tokio::time::Instant;

use crate::{
    config::EventsConfig,
    events::{Action, Events, Trigger},
    teamspeak::CurrentChannel,
};

const HOME: u64 = 1;
const ROOM: u64 = 2;
const OTHER: u64 = 3;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn config() -> EventsConfig {
    let mut config = EventsConfig::default();
    config.on_idle.delay = secs(600);
    config.on_alone.delay = secs(60);
    config.on_party.delay = secs(0);
    config
}

fn home(others: usize) -> CurrentChannel {
    CurrentChannel {
        id: HOME,
        at_home: true,
        others,
    }
}

fn room(id: u64, others: usize) -> CurrentChannel {
    CurrentChannel {
        id,
        at_home: false,
        others,
    }
}

struct Clock {
    start: Instant,
    events: Events,
}

impl Clock {
    fn new() -> Self {
        Self::with(config())
    }

    fn with(config: EventsConfig) -> Self {
        let start = Instant::now();
        let mut events = Events::new(&config, start);
        assert_eq!(events.channel(home(0), start), []);
        Self { start, events }
    }

    fn in_room(others: usize) -> Self {
        Self::in_room_with(config(), others)
    }

    fn in_room_with(config: EventsConfig, others: usize) -> Self {
        let mut clock = Self::with(config);
        clock.channel(0, room(ROOM, others));
        clock
    }

    fn done(&mut self, seconds: u64, trigger: Trigger) {
        let now = self.at(seconds);
        self.events.actions_done(trigger, now);
    }

    fn at(&self, seconds: u64) -> Instant {
        self.start + secs(seconds)
    }

    fn channel(&mut self, seconds: u64, channel: CurrentChannel) -> Vec<Action> {
        let now = self.at(seconds);
        self.events.channel(channel, now)
    }

    fn playing(&mut self, seconds: u64, playing: bool) {
        let now = self.at(seconds);
        self.events.playing(playing, now);
    }

    fn tick(&mut self, seconds: u64) -> Vec<Action> {
        let now = self.at(seconds);
        self.events.tick(now)
    }

    fn deadline(&self) -> Option<u64> {
        self.events
            .next_deadline()
            .map(|at| (at - self.start).as_secs())
    }
}

#[test]
fn nothing_happens_before_the_channel_is_known() {
    let start = Instant::now();
    let mut events = Events::new(&config(), start);
    assert_eq!(events.next_deadline(), None);
    assert_eq!(events.tick(start + secs(3600)), []);
}

#[test]
fn nothing_happens_at_home_without_music() {
    let mut clock = Clock::new();
    assert_eq!(clock.deadline(), None);
    assert_eq!(clock.tick(3600), []);
}

#[test]
fn moving_to_a_channel_with_people_runs_party() {
    let mut clock = Clock::new();
    assert_eq!(
        clock.channel(5, room(ROOM, 2)),
        [Action::Moved { home: false }, Action::Run(Trigger::Party)]
    );
}

#[test]
fn party_waits_for_its_delay() {
    let mut config = config();
    config.on_party.delay = secs(3);
    let start = Instant::now();
    let mut events = Events::new(&config, start);
    events.channel(home(0), start);
    assert_eq!(
        events.channel(room(ROOM, 1), start),
        [Action::Moved { home: false }]
    );
    assert_eq!(events.next_deadline(), Some(start + secs(3)));
    assert_eq!(events.tick(start + secs(3)), [Action::Run(Trigger::Party)]);
    assert_eq!(events.tick(start + secs(4)), []);
}

#[test]
fn party_is_skipped_when_everyone_leaves_first() {
    let mut config = config();
    config.on_party.delay = secs(3);
    let start = Instant::now();
    let mut events = Events::new(&config, start);
    events.channel(home(0), start);
    events.channel(room(ROOM, 1), start);
    assert_eq!(events.channel(room(ROOM, 0), start + secs(1)), []);
    assert_eq!(events.tick(start + secs(3)), []);
}

#[test]
fn someone_joining_the_alone_bot_runs_party() {
    let mut clock = Clock::in_room(0);
    assert_eq!(
        clock.channel(10, room(ROOM, 1)),
        [Action::Run(Trigger::Party)]
    );
}

#[test]
fn a_second_person_does_not_run_party() {
    let mut clock = Clock::in_room(1);
    assert_eq!(clock.channel(10, room(ROOM, 2)), []);
}

#[test]
fn no_party_while_music_plays() {
    let mut clock = Clock::in_room(0);
    clock.playing(1, true);
    assert_eq!(clock.channel(10, room(ROOM, 1)), []);
}

#[test]
fn no_party_at_home() {
    let mut clock = Clock::new();
    assert_eq!(clock.channel(10, home(1)), []);
}

fn with_party_delay() -> (Events, Instant) {
    let mut config = config();
    config.on_party.delay = secs(10);
    let start = Instant::now();
    let mut events = Events::new(&config, start);
    events.channel(home(0), start);
    (events, start)
}

#[test]
fn a_welcome_due_elsewhere_does_not_run_at_home() {
    let (mut events, start) = with_party_delay();
    events.channel(room(ROOM, 2), start);
    assert_eq!(
        events.channel(home(1), start + secs(1)),
        [Action::Moved { home: true }]
    );
    assert_eq!(events.tick(start + secs(10)), []);
    assert_eq!(events.tick(start + secs(20)), []);
}

#[test]
fn a_welcome_due_in_the_old_channel_starts_over_in_the_new_one() {
    let (mut events, start) = with_party_delay();
    events.channel(room(ROOM, 2), start);
    events.channel(room(OTHER, 1), start + secs(5));
    assert_eq!(events.tick(start + secs(10)), []);
    assert_eq!(events.tick(start + secs(15)), [Action::Run(Trigger::Party)]);
}

#[test]
fn a_move_to_an_empty_channel_cancels_the_welcome() {
    let (mut events, start) = with_party_delay();
    events.channel(room(ROOM, 2), start);
    events.channel(room(OTHER, 0), start + secs(1));
    assert_eq!(events.tick(start + secs(10)), []);
}

#[test]
fn alone_away_from_home_runs_after_the_delay() {
    let mut clock = Clock::in_room(0);
    assert_eq!(clock.deadline(), Some(60));
    assert_eq!(clock.tick(59), []);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    assert_eq!(clock.tick(3600), []);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn alone_counts_from_when_the_last_person_left() {
    let mut clock = Clock::in_room(1);
    clock.playing(0, true);
    assert_eq!(clock.channel(100, room(ROOM, 0)), []);
    assert_eq!(clock.deadline(), Some(160));
    assert_eq!(clock.tick(160), [Action::Run(Trigger::Alone)]);
}

#[test]
fn someone_coming_back_cancels_alone() {
    let mut clock = Clock::in_room(1);
    clock.playing(0, true);
    clock.channel(100, room(ROOM, 0));
    clock.channel(130, room(ROOM, 1));
    assert_eq!(clock.tick(160), []);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn idle_away_from_home_runs_after_the_delay() {
    let mut clock = Clock::in_room(1);
    assert_eq!(clock.deadline(), Some(600));
    assert_eq!(clock.tick(600), [Action::Run(Trigger::Idle)]);
    assert_eq!(clock.tick(3600), []);
}

#[test]
fn idle_counts_from_the_end_of_the_music() {
    let mut clock = Clock::in_room(1);
    clock.playing(10, true);
    assert_eq!(clock.deadline(), None);
    clock.playing(200, false);
    assert_eq!(clock.deadline(), Some(800));
    assert_eq!(clock.tick(799), []);
    assert_eq!(clock.tick(800), [Action::Run(Trigger::Idle)]);
}

#[test]
fn music_cancels_a_departure() {
    let mut clock = Clock::in_room(1);
    assert_eq!(clock.tick(600), [Action::Run(Trigger::Idle)]);
    clock.playing(605, true);
    clock.playing(700, false);
    assert_eq!(clock.deadline(), Some(1300));
}

#[test]
fn only_one_leaving_event_when_both_timers_run() {
    let mut clock = Clock::in_room(0);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    assert_eq!(clock.tick(600), []);
}

#[test]
fn both_due_at_once_run_one_event() {
    let mut config = config();
    config.on_idle.delay = secs(60);
    let start = Instant::now();
    let mut events = Events::new(&config, start);
    events.channel(home(0), start);
    events.channel(room(ROOM, 0), start);
    assert_eq!(events.tick(start + secs(60)), [Action::Run(Trigger::Alone)]);
}

#[test]
fn arriving_home_is_reported() {
    let mut clock = Clock::in_room(1);
    assert_eq!(clock.channel(50, home(0)), [Action::Moved { home: true }]);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn moving_between_rooms_restarts_the_idle_time() {
    let mut clock = Clock::in_room(1);
    clock.channel(500, room(OTHER, 1));
    assert_eq!(clock.tick(600), []);
    assert_eq!(clock.deadline(), Some(1100));
}

#[test]
fn moving_after_a_leaving_event_starts_over() {
    let mut clock = Clock::in_room(0);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    assert_eq!(
        clock.channel(65, room(OTHER, 2)),
        [Action::Moved { home: false }, Action::Run(Trigger::Party)]
    );
    assert_eq!(clock.deadline(), Some(665));
}

#[test]
fn alone_at_home_with_music_stops_it() {
    let mut clock = Clock::new();
    clock.playing(10, true);
    assert_eq!(clock.deadline(), Some(70));
    assert_eq!(clock.tick(70), [Action::StopAtHome]);
    assert_eq!(clock.tick(3600), []);
}

#[test]
fn listeners_at_home_keep_the_music() {
    let mut clock = Clock::new();
    clock.channel(0, home(2));
    clock.playing(10, true);
    assert_eq!(clock.deadline(), None);
    assert_eq!(clock.tick(3600), []);
}

#[test]
fn everyone_leaving_home_stops_the_music_later() {
    let mut clock = Clock::new();
    clock.channel(0, home(1));
    clock.playing(10, true);
    clock.channel(100, home(0));
    assert_eq!(clock.tick(160), [Action::StopAtHome]);
}

#[test]
fn stopping_at_home_does_not_repeat_without_new_music() {
    let mut clock = Clock::new();
    clock.playing(10, true);
    assert_eq!(clock.tick(70), [Action::StopAtHome]);
    clock.playing(70, false);
    assert_eq!(clock.tick(3600), []);
    clock.playing(4000, true);
    assert_eq!(clock.tick(4060), [Action::StopAtHome]);
}

fn staying() -> EventsConfig {
    let mut config = config();
    config.on_alone.move_to = None;
    config.on_idle.move_to = None;
    config
}

#[test]
fn a_move_that_did_not_happen_is_retried_after_the_delay() {
    let mut clock = Clock::in_room(0);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    clock.done(61, Trigger::Alone);
    assert_eq!(clock.deadline(), Some(121));
    assert_eq!(clock.tick(121), [Action::Run(Trigger::Alone)]);
}

#[test]
fn arriving_home_after_the_actions_starts_over() {
    let mut clock = Clock::in_room(0);
    clock.tick(60);
    clock.done(60, Trigger::Alone);
    assert_eq!(clock.channel(61, home(0)), [Action::Moved { home: true }]);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn a_leaving_event_blocks_others_until_its_actions_are_done() {
    let mut clock = Clock::in_room_with(staying(), 0);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    assert_eq!(clock.channel(62, room(ROOM, 1)), []);
    assert_eq!(clock.channel(63, room(ROOM, 0)), []);
    clock.done(64, Trigger::Alone);
    assert_eq!(
        clock.channel(70, room(ROOM, 1)),
        [Action::Run(Trigger::Party)]
    );
}

#[test]
fn a_leaving_event_without_a_move_does_not_repeat() {
    let mut clock = Clock::in_room_with(staying(), 0);
    assert_eq!(clock.tick(60), [Action::Run(Trigger::Alone)]);
    clock.done(60, Trigger::Alone);
    assert_eq!(clock.tick(599), []);
    assert_eq!(clock.deadline(), Some(600));
    assert_eq!(clock.tick(600), [Action::Run(Trigger::Idle)]);
    clock.done(600, Trigger::Idle);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn done_after_music_took_over_changes_nothing() {
    let mut clock = Clock::in_room(1);
    assert_eq!(clock.tick(600), [Action::Run(Trigger::Idle)]);
    clock.playing(601, true);
    clock.done(602, Trigger::Idle);
    assert_eq!(clock.deadline(), None);
}

#[test]
fn done_for_a_party_changes_nothing() {
    let mut clock = Clock::in_room(1);
    clock.done(5, Trigger::Party);
    assert_eq!(clock.deadline(), Some(600));
}

#[test]
fn connect_runs_after_its_delay_and_again_after_a_reconnect() {
    let mut config = config();
    config.on_connect.delay = secs(5);
    let mut clock = Clock::with(config);
    assert_eq!(clock.events.connected(clock.at(0)), []);
    assert_eq!(clock.deadline(), Some(5));
    assert_eq!(clock.tick(5), [Action::Run(Trigger::Connect)]);
    assert_eq!(clock.tick(6), []);
    assert_eq!(clock.events.connected(clock.at(100)), []);
    assert_eq!(clock.tick(105), [Action::Run(Trigger::Connect)]);
}

#[test]
fn connect_does_not_wait_for_the_channel() {
    let start = Instant::now();
    let mut events = Events::new(&config(), start);
    assert_eq!(events.connected(start), [Action::Run(Trigger::Connect)]);
}

#[test]
fn song_start_without_a_delay_runs_at_once() {
    let mut clock = Clock::in_room(1);
    let now = clock.at(10);
    assert_eq!(
        clock.events.song_started(now),
        [Action::Run(Trigger::SongStart)]
    );
}

#[test]
fn a_newer_song_replaces_the_waiting_song_start() {
    let mut config = config();
    config.on_song_start.delay = secs(5);
    let mut clock = Clock::in_room_with(config, 1);
    assert_eq!(clock.events.song_started(clock.at(10)), []);
    assert_eq!(clock.events.song_started(clock.at(12)), []);
    assert_eq!(clock.deadline(), Some(17));
    assert_eq!(clock.tick(15), []);
    assert_eq!(clock.tick(17), [Action::Run(Trigger::SongStart)]);
}
