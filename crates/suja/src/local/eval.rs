use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Deserialize;
use suja_agent::Agent;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct CaseFile {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    prompt: String,
    expect: Expect,
}

#[derive(Deserialize, PartialEq, Debug)]
enum Expect {
    Tool(String),
    NoTool,
}

pub async fn eval(agent: &Agent, cases: &Path) -> Result<()> {
    let file: CaseFile = ron::from_str(&std::fs::read_to_string(cases).context("read cases")?)
        .context("parse cases")?;
    score(agent, &file).await
}

async fn score(agent: &Agent, file: &CaseFile) -> Result<()> {
    let mut passed = 0usize;
    let mut first_token_total = 0u128;
    for case in &file.cases {
        let started = Instant::now();
        let report = agent
            .first_action(&case.prompt, CancellationToken::new())
            .await?;
        first_token_total += started.elapsed().as_millis();
        let spoken = report.spoken;
        let got = report
            .tool_calls
            .first()
            .map_or(Expect::NoTool, |call| Expect::Tool(call.name.clone()));
        let ok = got == case.expect;
        passed += usize::from(ok);
        println!(
            "{} {:<55} expected {:?}, got {:?}{}",
            if ok { "PASS" } else { "FAIL" },
            case.prompt,
            case.expect,
            got,
            if ok {
                String::new()
            } else {
                format!("  | said: {}", spoken.trim())
            }
        );
    }
    println!(
        "\n{passed}/{} passed, mean decision time {} ms (no tool was executed)",
        file.cases.len(),
        first_token_total / file.cases.len().max(1) as u128
    );
    Ok(())
}
