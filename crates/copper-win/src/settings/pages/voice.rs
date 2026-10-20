//! Voice (`Voice/VoiceSettings.swift`): dictation in the agent pane. The
//! Mac's speech model runs on Core ML; Copper for Windows has no speech
//! engine yet, so every row says so, faded, and nothing can be turned on
//! (no download, no microphone, no credits for a model that isn't here).

use super::not_yet;
use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;

/// Voice: dictation in the agent pane.
pub(super) struct Voice;

impl Page for Voice {
    fn id(&self) -> &'static str {
        "voice"
    }
    fn title(&self) -> &'static str {
        "Voice"
    }
    fn blurb(&self) -> &'static str {
        "Dictation in the agent pane: the speech model, and how you talk to it."
    }
    fn glyph(&self) -> char {
        glyph::MIC
    }

    fn draw(&self, _shell: &Shell) -> Form {
        let mut f = Form::default();
        let sec = f.section("Voice on this PC");
        not_yet(
            sec,
            "Voice",
            "Not yet on Windows — Copper for Windows has no speech model, so it never listens",
        )
        .anchor("voice.enabled");
        let sec = f.section("Talking to the agent");
        not_yet(
            sec,
            "Talk",
            "Hold to talk, or press to start and stop — once Voice is here",
        )
        .anchor("voice.trigger");
        not_yet(
            sec,
            "When you stop",
            "Insert into the message, or send right away — once Voice is here",
        )
        .anchor("voice.finish");
        let sec = f.section("Listen");
        not_yet(
            sec,
            "Let agents on this PC read the Listen transcript",
            "Off — there is no transcript without Voice",
        )
        .anchor("voice.share");
        f
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}
}
