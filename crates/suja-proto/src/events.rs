use crownos_ipc::{EventMsg, Message, postcard};

use crate::protocol::{
    AssistantText, Event, Failure, FinalTranscript, MicLevel, PartialTranscript, StateChanged,
    ToolCall, ToolResult, TurnFinished,
};

pub fn decode_event(message: &Message) -> Option<Event> {
    macro_rules! try_decode {
        ($($event:ident),* $(,)?) => {
            $(
                if message.selector == <$event as EventMsg>::SELECTOR {
                    return postcard::from_bytes::<$event>(&message.payload).ok().map(Event::$event);
                }
            )*
        };
    }
    try_decode!(
        StateChanged,
        MicLevel,
        PartialTranscript,
        FinalTranscript,
        AssistantText,
        ToolCall,
        ToolResult,
        TurnFinished,
        Failure,
    );
    None
}
