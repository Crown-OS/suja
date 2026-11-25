use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::json;
use suja_agent::{Agent, HANDOFF_TOOL, TurnEvent};
use suja_model::{
    CancellationToken, ChatModel, Entry, Expert, Finish, FinishReason, ModelCapabilities,
    ModelError, ModelEvent, ModelStream, ToolCall, TurnRequest, Usage,
};
use suja_tools::Registry;
use tokio::sync::mpsc;

/// Answers each request with the next scripted reply and keeps the requests.
#[derive(Default)]
struct ScriptedModel {
    replies: Mutex<VecDeque<Vec<Result<ModelEvent, ModelError>>>>,
    requests: Mutex<Vec<TurnRequest>>,
}

impl ScriptedModel {
    fn with_replies(replies: Vec<Vec<ModelEvent>>) -> Arc<Self> {
        Self::with_results(
            replies
                .into_iter()
                .map(|reply| reply.into_iter().map(Ok).collect())
                .collect(),
        )
    }

    fn with_results(replies: Vec<Vec<Result<ModelEvent, ModelError>>>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            requests: Mutex::default(),
        })
    }

    fn requests(&self) -> Vec<TurnRequest> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

impl ChatModel for ScriptedModel {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            context_tokens: 100_000,
            parallel_tool_calls: true,
            runs_locally: true,
        }
    }

    fn stream(&self, request: TurnRequest, _: CancellationToken) -> ModelStream {
        if let Ok(mut requests) = self.requests.lock() {
            requests.push(request);
        }
        let reply = self
            .replies
            .lock()
            .ok()
            .and_then(|mut replies| replies.pop_front())
            .unwrap_or_default();
        let (events, stream) = mpsc::unbounded_channel();
        for event in reply {
            let _ = events.send(event);
        }
        stream
    }
}

fn finished(reason: FinishReason) -> ModelEvent {
    ModelEvent::Finished(Finish {
        reason,
        usage: Usage::default(),
        backend_data: None,
    })
}

fn agent(base: &Arc<ScriptedModel>, expert: &Arc<ScriptedModel>) -> Agent {
    let experts = vec![Expert {
        name: "claude".into(),
        use_for: "hard reasoning".into(),
        model: expert.clone(),
    }];
    Agent::new(base.clone(), experts, Arc::new(Registry::default()))
}

async fn turn(agent: &Agent, text: &str) -> Vec<TurnEvent> {
    let mut events = Vec::new();
    let result = agent
        .turn(text, None, CancellationToken::new(), |event| {
            events.push(event)
        })
        .await;
    assert!(result.is_ok(), "{result:?}");
    events
}

fn has_handoff_tool(request: &TurnRequest) -> bool {
    request.tools.iter().any(|tool| tool.name == HANDOFF_TOOL)
}

#[tokio::test]
async fn the_base_hands_off_and_the_expert_answer_stays_in_the_conversation() {
    let handoff = ToolCall {
        id: "call_1".into(),
        name: HANDOFF_TOOL.into(),
        arguments: json!({"expert": "claude", "task": "plan three days in Kyoto"}),
    };
    let base = ScriptedModel::with_replies(vec![
        vec![
            ModelEvent::ToolCall(handoff),
            finished(FinishReason::ToolCall),
        ],
        vec![
            ModelEvent::Text("Five days.".into()),
            finished(FinishReason::EndOfTurn),
        ],
    ]);
    let expert = ScriptedModel::with_replies(vec![vec![
        ModelEvent::Text("Start at Fushimi Inari.".into()),
        finished(FinishReason::EndOfTurn),
    ]]);
    let agent = agent(&base, &expert);

    let events = turn(&agent, "plan a trip to Kyoto").await;
    assert!(events.contains(&TurnEvent::Handoff {
        expert: "claude".into(),
        task: "plan three days in Kyoto".into()
    }));
    assert!(events.contains(&TurnEvent::Text("Start at Fushimi Inari.".into())));
    assert_eq!(
        events.last(),
        Some(&TurnEvent::Finished(FinishReason::EndOfTurn))
    );

    let expert_request = &expert.requests()[0];
    assert!(!has_handoff_tool(expert_request));
    assert!(matches!(
        expert_request.transcript.last(),
        Some(Entry::User(text)) if text.contains("plan three days in Kyoto")
    ));

    turn(&agent, "how long should I stay").await;
    let follow_up = &base.requests()[1];
    assert!(has_handoff_tool(follow_up));
    assert!(follow_up.transcript.iter().any(|entry| matches!(
        entry,
        Entry::ToolResults(results) if results[0].call_id == "call_1" && results[0].content == "Start at Fushimi Inari."
    )));
}

#[tokio::test]
async fn ask_claude_goes_straight_to_the_expert() {
    let base = ScriptedModel::with_replies(Vec::new());
    let expert = ScriptedModel::with_replies(vec![vec![
        ModelEvent::Text("Because of Rayleigh scattering.".into()),
        finished(FinishReason::EndOfTurn),
    ]]);
    let agent = agent(&base, &expert);

    let events = turn(&agent, "ask Claude why is the sky blue").await;
    assert!(base.requests().is_empty());
    assert!(events.contains(&TurnEvent::Handoff {
        expert: "claude".into(),
        task: "why is the sky blue".into()
    }));
    assert!(events.contains(&TurnEvent::Text("Because of Rayleigh scattering.".into())));
}

#[tokio::test]
async fn a_full_context_keeps_only_the_current_turn_and_tries_again() {
    let base = ScriptedModel::with_results(vec![
        vec![
            Ok(ModelEvent::Text("Hello.".into())),
            Ok(finished(FinishReason::EndOfTurn)),
        ],
        vec![Err(ModelError::ContextFull)],
        vec![
            Ok(ModelEvent::Text("Noon.".into())),
            Ok(finished(FinishReason::EndOfTurn)),
        ],
    ]);
    let agent = Agent::new(base.clone(), Vec::new(), Arc::new(Registry::default()));
    turn(&agent, "hi").await;
    let events = turn(&agent, "what time is it").await;
    assert!(events.contains(&TurnEvent::Text("Noon.".into())));
    let retry = &base.requests()[2];
    assert_eq!(retry.transcript.len(), 1);
    assert!(matches!(&retry.transcript[0], Entry::User(text) if text.ends_with("what time is it")));
}
