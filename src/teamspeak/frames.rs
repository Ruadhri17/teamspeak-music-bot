//! Which frame from the audio thread goes out next, apart from the connection so it can be
//! tested without a server.
//!
//! Before the bot starts speaking (a new track from silence, or after the buffer ran dry)
//! it gathers `audio.start_buffer` of audio, so an uneven input does not break up the
//! start. A stream shorter than that starts as soon as its end is in. Frames of stopped
//! streams are dropped on the way, and the end of a stream that went out is collected for
//! the bot.

use std::collections::VecDeque;

use tokio::sync::mpsc;

use crate::audio::{FRAME_SAMPLES, Frame};

pub type Pcm = Box<[f32; FRAME_SAMPLES]>;

pub struct Frames {
    channel: mpsc::Receiver<Frame>,
    /// Taken out of the channel but not sent yet: gathered for the start, or kept during
    /// a pause so the stopped streams behind it can be dropped.
    held: VecDeque<Frame>,
    /// Frames to gather before speaking starts, at least one.
    start: usize,
    /// Streams whose last frame went out, oldest first, until `take_ended`.
    ended: Vec<u64>,
    /// The stream of the last frame that went out while its end has not, for telling a
    /// buffer that ran dry from one whose audio is over.
    open: Option<u64>,
}

impl Frames {
    pub fn new(channel: mpsc::Receiver<Frame>, start: usize) -> Self {
        Self {
            channel,
            held: VecDeque::new(),
            start: start.max(1),
            ended: Vec::new(),
            open: None,
        }
    }

    /// The frame to send now, `None` when there is nothing to send yet. `speaking` is
    /// whether the previous tick sent a frame: without it the start buffer must fill first.
    /// The end of a stream reached on the way is collected, and the next stream's first
    /// frame still goes out in this call, so tracks follow each other without a gap.
    pub fn next(&mut self, min_stream_id: u64, speaking: bool) -> Option<Pcm> {
        self.drop_stopped(min_stream_id);
        if !speaking && !self.gather(min_stream_id) {
            return None;
        }
        while let Some(frame) = self.pop(min_stream_id) {
            match frame {
                Frame::Audio { stream, pcm } => {
                    self.open = Some(stream);
                    return Some(pcm);
                }
                Frame::End { stream } => {
                    if self.open == Some(stream) {
                        self.open = None;
                    }
                    self.ended.push(stream);
                }
            }
        }
        None
    }

    /// A stream that still plays has sent frames but not its end yet: an empty buffer now
    /// is an underrun, not the end of the audio.
    pub fn mid_stream(&self, min_stream_id: u64) -> bool {
        self.open.is_some_and(|stream| stream >= min_stream_id)
    }

    /// Nothing is sent (a pause, a reconnect). The frames of stopped streams are dropped
    /// and one frame is kept, so audio threads blocked on the full channel can see their
    /// stop and end.
    pub fn hold(&mut self, min_stream_id: u64) {
        self.drop_stopped(min_stream_id);
        if self.held.is_empty()
            && let Some(frame) = self.pop(min_stream_id)
        {
            self.held.push_back(frame);
        }
    }

    /// The streams heard to their end since the last call, oldest first.
    pub fn take_ended(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.ended)
    }

    /// Moves frames from the channel into `held` until the start buffer is full or a
    /// stream's end is in. `true` when sending may start.
    fn gather(&mut self, min_stream_id: u64) -> bool {
        let mut audio = 0;
        let mut ended = false;
        for frame in &self.held {
            match frame {
                Frame::Audio { .. } => audio += 1,
                Frame::End { .. } => ended = true,
            }
        }
        while audio < self.start && !ended {
            match self.channel.try_recv() {
                Ok(frame) if frame.stream() >= min_stream_id => {
                    match frame {
                        Frame::Audio { .. } => audio += 1,
                        Frame::End { .. } => ended = true,
                    }
                    self.held.push_back(frame);
                }
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        true
    }

    /// The oldest frame of a stream that still plays, held ones first.
    fn pop(&mut self, min_stream_id: u64) -> Option<Frame> {
        std::iter::from_fn(|| {
            self.held
                .pop_front()
                .or_else(|| self.channel.try_recv().ok())
        })
        .find(|frame| frame.stream() >= min_stream_id)
    }

    /// Stream IDs only go up, so the stopped ones are all at the front.
    fn drop_stopped(&mut self, min_stream_id: u64) {
        while self
            .held
            .front()
            .is_some_and(|frame| frame.stream() < min_stream_id)
        {
            self.held.pop_front();
        }
    }
}
