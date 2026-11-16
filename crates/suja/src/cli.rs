use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crownos_ipc::adapter::tokio::AsyncClient;
use suja_proto::protocol;

use crate::output::print_event;

#[derive(Parser)]
#[command(name = "suja", about = "Talk to the suja voice assistant daemon")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Start, stop or toggle listening")]
    Talk { action: TalkAction },
    #[command(about = "Interrupt the current answer")]
    Cancel,
    #[command(about = "Speak a sentence through the daemon's voice")]
    Say { text: Vec<String> },
    #[command(about = "Run a turn from typed text instead of speech")]
    Ask { text: Vec<String> },
    #[command(about = "Show daemon state and loaded models")]
    Status,
    #[command(about = "Stream daemon events to stdout")]
    Watch,
    #[cfg(feature = "local")]
    #[command(about = "Text chat with the local model, no daemon needed")]
    Chat {
        #[arg(long)]
        model: Option<std::path::PathBuf>,
        prompt: Vec<String>,
    },
    #[cfg(feature = "local")]
    #[command(about = "Score tool selection on a RON case file")]
    Eval {
        #[arg(long)]
        model: Option<std::path::PathBuf>,
        cases: std::path::PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum TalkAction {
    Start,
    Stop,
    Toggle,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        #[cfg(feature = "local")]
        match &self.command {
            Command::Chat { model, prompt } => {
                return crate::local::chat(model.clone(), prompt.join(" ")).await;
            }
            Command::Eval { model, cases } => {
                return crate::local::eval(model.clone(), cases).await;
            }
            _ => {}
        }
        let mut client = connect()?;
        match self.command {
            Command::Talk { action } => match action {
                TalkAction::Start => notify(&mut client, &protocol::talk_start {}).await,
                TalkAction::Stop => notify(&mut client, &protocol::talk_stop {}).await,
                TalkAction::Toggle => notify(&mut client, &protocol::talk_toggle {}).await,
            },
            Command::Cancel => notify(&mut client, &protocol::cancel {}).await,
            Command::Say { text } => {
                let text = text.join(" ");
                notify(&mut client, &protocol::say { text }).await
            }
            Command::Ask { text } => {
                let text = text.join(" ");
                notify(&mut client, &protocol::ask { text }).await
            }
            Command::Status => {
                let status = client
                    .call::<protocol::status>(&protocol::status {}, Vec::new())
                    .await?;
                println!("{status:#?}");
                Ok(())
            }
            Command::Watch => watch(client).await,
            #[cfg(feature = "local")]
            Command::Chat { .. } | Command::Eval { .. } => Ok(()),
        }
    }
}

fn connect() -> Result<AsyncClient> {
    AsyncClient::connect(protocol::SERVICE).context("sujad is not running")
}

async fn notify<M: crownos_ipc::MethodMsg<Args = M>>(
    client: &mut AsyncClient,
    args: &M,
) -> Result<()> {
    client.notify::<M>(args, Vec::new()).await?;
    Ok(())
}

async fn watch(mut client: AsyncClient) -> Result<()> {
    client.subscribe::<protocol::StateChanged>().await?;
    client.subscribe::<protocol::MicLevel>().await?;
    client.subscribe::<protocol::PartialTranscript>().await?;
    client.subscribe::<protocol::FinalTranscript>().await?;
    client.subscribe::<protocol::AssistantText>().await?;
    client.subscribe::<protocol::ToolCall>().await?;
    client.subscribe::<protocol::ToolResult>().await?;
    client.subscribe::<protocol::TurnFinished>().await?;
    client.subscribe::<protocol::Failure>().await?;
    loop {
        while let Some(message) = client.client_mut().next_event_raw() {
            if let Some(event) = suja_proto::decode_event(&message) {
                print_event(&event);
            }
        }
        if client.client().is_closed() {
            anyhow::bail!("sujad closed the connection");
        }
        client.pump().await?;
    }
}
