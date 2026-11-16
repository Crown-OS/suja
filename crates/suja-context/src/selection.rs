use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

pub async fn primary_selection(budget: Duration) -> Option<String> {
    let output = Command::new("wl-paste")
        .args(["--primary", "--no-newline", "--type", "text/plain"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let output = tokio::time::timeout(budget, output).await.ok()?.ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
