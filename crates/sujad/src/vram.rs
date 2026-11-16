use anyhow::{Context, Result, bail};
use tokio::process::Command;

pub async fn free_mib() -> Result<u64> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
        .await
        .context("run nvidia-smi")?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
        .context("parse nvidia-smi output")
}

pub async fn preflight(min_free_mib: u64) -> Result<()> {
    match free_mib().await {
        Ok(free) if free < min_free_mib => bail!(
            "only {free} MiB of VRAM free, need {min_free_mib}; stop crowndictator's GPU mode or other GPU apps"
        ),
        Ok(free) => {
            tracing::info!(free_mib = free, "vram preflight ok");
            Ok(())
        }
        Err(error) => {
            tracing::warn!(%error, "vram preflight skipped");
            Ok(())
        }
    }
}
