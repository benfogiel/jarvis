mod microphone;
mod playback;
mod speaker;
mod transcriber;

use crate::control::{Conversation, Phase};
use crate::llm::{self, ChatModel, Role, Turn};
use bevy::prelude::*;
use microphone::Microphone;
use speaker::Speaker;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::thread;
use transcriber::Transcriber;

const INSTRUCTIONS: &str = "You are Jarvis, a helpful voice assistant who happens to talk like an \
old-school British jungle and drum and bass MC: a London veteran of the nineties raves and pirate \
radio, warm and quick-witted. That character shows only in your choice of words and your rhythm. \
Being useful comes first. Answer the question that was asked, directly and accurately, starting \
with the answer itself. Do not add hype, shout-outs, rhymes, intros, sign-offs or anything else \
that is not part of the answer, and never go off on a tangent. A word or two of the scene's London \
and Jamaican-British slang in a reply is plenty. \
Everything you write is read aloud by a speech synthesiser, and what you receive is a transcription \
of speech that may contain recognition mistakes. Write plain sentences with no markdown, lists, \
emoji, stage directions or symbols that would sound odd spoken. Keep answers to a sentence or two \
unless asked for more.";
const UNAVAILABLE: &str = "Hold tight, the signal dropped out. I couldn't get an answer just now.";
/// Older turns are forgotten so an orb left running does not send an ever longer conversation.
const REMEMBERED_TURNS: usize = 40;

/// What the conversation thread shares with the render loop.
#[derive(Default)]
struct Signals {
    phase: AtomicU8,
    loudness: AtomicU32,
}

impl Signals {
    fn enter(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    fn phase(&self) -> Phase {
        [Phase::Listening, Phase::Thinking, Phase::Speaking]
            .into_iter()
            .find(|phase| *phase as u8 == self.phase.load(Ordering::Relaxed))
            .unwrap_or_default()
    }
}

#[derive(Resource)]
struct VoiceLink(Arc<Signals>);

pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        let signals = Arc::new(Signals::default());
        app.insert_resource(VoiceLink(signals.clone()))
            .add_systems(PreUpdate, mirror_signals);
        // The microphone stream cannot move between threads, so everything that touches it lives on
        // this one.
        thread::spawn(move || {
            if let Err(error) = converse(&signals) {
                error!("voice is unavailable: {error}");
            }
        });
    }
}

fn mirror_signals(link: Res<VoiceLink>, mut conversation: ResMut<Conversation>) {
    conversation.phase = link.0.phase();
    conversation.loudness = f32::from_bits(link.0.loudness.load(Ordering::Relaxed));
}

fn converse(signals: &Signals) -> Result<(), String> {
    let model = llm::from_environment();
    if let Err(error) = &model {
        error!("{error}");
    }
    let speaker = Speaker::from_environment().unwrap_or_else(|error| {
        error!("{error}; falling back to the system voice");
        Speaker::System
    });
    let transcriber = Transcriber::load()?;
    let mut microphone = Microphone::open()?;
    let mut turns: Vec<Turn> = Vec::new();
    info!("listening");

    loop {
        signals.enter(Phase::Listening);
        let utterance = microphone.next_utterance(transcriber::SAMPLE_RATE, |loudness| {
            signals.loudness.store(loudness.to_bits(), Ordering::Relaxed);
        })?;

        signals.enter(Phase::Thinking);
        let heard = transcriber.transcribe(&utterance)?;
        if heard.is_empty() {
            continue;
        }
        info!("heard: {heard}");
        let reply = answer(&model, &mut turns, heard);
        info!("reply: {reply}");

        signals.enter(Phase::Speaking);
        if let Err(error) = speaker.speak(&reply) {
            error!("{error}");
        }
        microphone.discard_pending();
    }
}

fn answer(model: &Result<Box<dyn ChatModel>, String>, turns: &mut Vec<Turn>, heard: String) -> String {
    turns.push(Turn { role: Role::User, text: heard });
    let reply = model.as_ref().map_err(String::clone).and_then(|model| model.reply(INSTRUCTIONS, turns));
    match reply {
        Ok(reply) => {
            turns.push(Turn { role: Role::Assistant, text: reply.clone() });
            // Dropping a whole exchange at a time keeps the history starting on a user turn.
            while turns.len() > REMEMBERED_TURNS {
                turns.drain(..2);
            }
            reply
        }
        Err(error) => {
            error!("{error}");
            turns.pop();
            UNAVAILABLE.to_string()
        }
    }
}
