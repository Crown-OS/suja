use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use suja_agent::{Agent, TurnEvent};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use super::setup::load_agent;

pub async fn chat(model: Option<PathBuf>, prompt: String) -> Result<()> {
    let agent = load_agent(model).await?;
    let result = converse(&agent, prompt).await;
    agent.worker().shutdown();
    result
}

async fn converse(agent: &Agent, prompt: String) -> Result<()> {
    if !prompt.trim().is_empty() {
        return run_turn(agent, &prompt).await;
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        print!("you: ");
        std::io::stdout().flush()?;
        let Some(line) = lines.next_line().await? else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        run_turn(agent, &line).await?;
    }
    Ok(())
}

async fn run_turn(agent: &Agent, text: &str) -> Result<()> {
    let started = Instant::now();
    let mut first_token = None;
    let mut out = std::io::stdout();
    print!("suja: ");
    agent
        .turn(text, None, CancellationToken::new(), |event| {
            first_token.get_or_insert_with(|| started.elapsed());
            match event {
                TurnEvent::Text(t) => {
                    let _ = write!(out, "{t}");
                }
                TurnEvent::ToolCall(call) => {
                    let _ = write!(out, "\n  [call {}({})]\n", call.name, call.arguments);
                }
                TurnEvent::ToolResult { name, outcome } => {
                    let _ = writeln!(
                        out,
                        "  [{name} {}: {}]",
                        if outcome.ok { "ok" } else { "failed" },
                        outcome.spoken_summary
                    );
                }
                TurnEvent::NeedsConfirmation(call) => {
                    let _ = writeln!(out, "  [waiting for a yes before running {}]", call.name);
                }
                TurnEvent::Finished(_) => {}
            }
            let _ = out.flush();
        })
        .await?;
    let stats = agent.worker().stats().await?;
    eprintln!(
        "\n  first token {} ms, total {} ms, context {} tokens",
        first_token.map_or(0, |d| d.as_millis()),
        started.elapsed().as_millis(),
        stats.tokens_in_context
    );
    Ok(())
}
