use tokio::process::Command;

use crate::error::LlmError;

/// Free memory of the first NVIDIA GPU, from `nvidia-smi`.
pub async fn free_vram_mib() -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
}

/// Fails when less than `min_free_mib` of GPU memory is free. Without `nvidia-smi` it passes.
pub async fn preflight(min_free_mib: u64) -> Result<(), LlmError> {
    match free_vram_mib().await {
        Some(free) if free < min_free_mib => Err(LlmError::NotEnoughVram {
            free_mib: free,
            needed_mib: min_free_mib,
        }),
        Some(free) => {
            tracing::info!(free_mib = free, "vram preflight ok");
            Ok(())
        }
        None => {
            tracing::warn!("vram preflight skipped: nvidia-smi gave no answer");
            Ok(())
        }
    }
}
