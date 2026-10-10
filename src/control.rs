use bevy::prelude::*;

const ACTIVITY_RESPONSIVENESS: f32 = 3.4;
const SPIN_RESPONSIVENESS: f32 = 2.2;
/// The glow rises faster than it falls, so it catches the start of a syllable and then trails off
/// instead of flickering between words.
const GLOW_ATTACK: f32 = 28.0;
const GLOW_RELEASE: f32 = 6.0;
const REVEAL_SECONDS: f32 = 1.6;
const CONCEAL_SECONDS: f32 = 0.9;

// Nothing drives the conversation in the browser yet, so there it never leaves `Listening`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Listening,
    Thinking,
    Speaking,
}

impl Phase {
    fn activity(self) -> f32 {
        match self {
            Phase::Listening => 0.30,
            Phase::Thinking => 1.0,
            Phase::Speaking => 0.60,
        }
    }

    fn spin_per_second(self) -> f32 {
        match self {
            Phase::Listening => 0.13,
            Phase::Thinking => 1.9,
            Phase::Speaking => 0.30,
        }
    }
}

/// Where the conversation stands, and how loud the voice being heard is: zero for silence, one for
/// a raised voice.
#[derive(Resource, Default)]
pub struct Conversation {
    pub phase: Phase,
    pub loudness: f32,
}

/// How awake the orb is: `target` follows the conversation immediately, `eased` is what the shader
/// sees.
#[derive(Resource)]
pub struct Activity {
    pub target: f32,
    pub eased: f32,
}

impl Activity {
    pub fn new(amount: f32) -> Self {
        Self {
            target: amount,
            eased: amount,
        }
    }
}

/// How brightly the orb is lit by the voice it is hearing.
#[derive(Resource, Default)]
pub struct Glow(pub f32);

/// Whether the orb should be showing its image, and how far it has materialised towards that.
#[derive(Resource, Default)]
pub struct Reveal {
    pub showing: bool,
    pub amount: f32,
}

#[derive(Resource, Default)]
pub struct Spin {
    yaw: f32,
    per_second: f32,
}

impl Spin {
    pub fn orientation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }
}

fn blend(responsiveness: f32, time: &Time) -> f32 {
    1.0 - (-responsiveness * time.delta_secs()).exp()
}

pub fn follow_conversation(conversation: Res<Conversation>, mut activity: ResMut<Activity>) {
    activity.target = conversation.phase.activity();
}

pub fn ease_activity(time: Res<Time>, mut activity: ResMut<Activity>) {
    activity.eased += (activity.target - activity.eased) * blend(ACTIVITY_RESPONSIVENESS, &time);
}

pub fn ease_glow(time: Res<Time>, conversation: Res<Conversation>, mut glow: ResMut<Glow>) {
    let heard = match conversation.phase {
        Phase::Listening => conversation.loudness,
        Phase::Thinking | Phase::Speaking => 0.0,
    };
    let responsiveness = if heard > glow.0 { GLOW_ATTACK } else { GLOW_RELEASE };
    glow.0 += (heard - glow.0) * blend(responsiveness, &time);
}

/// The orb always turns a little so it never looks frozen, and winds up while it is thinking.
pub fn turn(time: Res<Time>, conversation: Res<Conversation>, mut spin: ResMut<Spin>) {
    let wanted = conversation.phase.spin_per_second();
    spin.per_second += (wanted - spin.per_second) * blend(SPIN_RESPONSIVENESS, &time);
    spin.yaw += spin.per_second * time.delta_secs();
}

/// Moves at a fixed rate rather than easing, so materialising always takes the same time however
/// far along it was when toggled.
pub fn advance_reveal(time: Res<Time>, mut reveal: ResMut<Reveal>) {
    let (goal, seconds) = if reveal.showing {
        (1.0, REVEAL_SECONDS)
    } else {
        (0.0, CONCEAL_SECONDS)
    };
    let step = time.delta_secs() / seconds;
    let advanced = if goal > reveal.amount {
        (reveal.amount + step).min(goal)
    } else {
        (reveal.amount - step).max(goal)
    };
    if advanced != reveal.amount {
        reveal.amount = advanced;
    }
}
