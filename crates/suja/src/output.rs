use std::io::Write;

use suja_proto::protocol::Event;

pub fn print_event(event: &Event) {
    let mut out = std::io::stdout().lock();
    let _ = match event {
        Event::StateChanged(e) => writeln!(out, "state: {:?}", e.state),
        Event::MicLevel(_) => Ok(()),
        Event::PartialTranscript(e) => write!(out, "\rheard: {}", e.text),
        Event::FinalTranscript(e) => writeln!(out, "\ryou: {}", e.text),
        Event::AssistantText(e) => write!(out, "{}", e.delta),
        Event::ToolCall(e) => writeln!(out, "\ntool {}({})", e.name, e.arguments),
        Event::ToolResult(e) => writeln!(
            out,
            "tool {} {}: {}",
            e.name,
            if e.ok { "ok" } else { "failed" },
            e.summary
        ),
        Event::TurnFinished(e) => writeln!(
            out,
            "\nturn {}: first audio {} ms after endpoint",
            e.timings.turn_id,
            e.timings.endpoint_to_first_audio_ms()
        ),
        Event::Failure(e) => writeln!(out, "error: {}", e.message),
        Event::HotkeyTapped(_) => writeln!(out, "hotkey tapped"),
    };
    let _ = out.flush();
}
