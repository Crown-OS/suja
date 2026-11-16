use std::time::Duration;

use crownos_clipboard::protocol;
use crownos_ipc::adapter::tokio::AsyncClient;

pub async fn latest_preview(budget: Duration) -> Option<String> {
    let fetch = async {
        let mut client = AsyncClient::connect(protocol::SERVICE).ok()?;
        let entries = client
            .call::<protocol::list>(&protocol::list {}, Vec::new())
            .await
            .ok()?;
        entries.into_iter().next().map(|entry| entry.preview)
    };
    tokio::time::timeout(budget, fetch).await.ok().flatten()
}
