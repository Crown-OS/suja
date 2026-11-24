mod events;
mod state;
mod turn;

pub use events::decode_event;
pub use state::{AssistantState, Status};
pub use turn::TurnTimings;

crownos_ipc::protocol! {
    pub mod protocol: "suja" {
        notify talk_start();
        notify talk_stop();
        notify talk_toggle();
        notify cancel();
        notify say(text: String);
        notify ask(text: String);
        fn status() -> Status;

        event StateChanged { state: AssistantState };
        event MicLevel { level: f32 };
        event PartialTranscript { text: String };
        event FinalTranscript { text: String };
        event AssistantText { delta: String };
        event ToolCall { name: String, arguments: String };
        event ToolResult { name: String, ok: bool, summary: String };
        event TurnFinished { timings: TurnTimings };
        event Failure { message: String };
        event HotkeyTapped {};
    }
}
