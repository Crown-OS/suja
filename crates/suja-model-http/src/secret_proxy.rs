use std::time::{Duration, Instant};

use crownos_ipc::adapter::tokio::AsyncClient;
use secrets_core::SecretName;
use secrets_protocol::secrets;
use secrets_protocol::{ProxySession, ProxySessionReply};

use crate::credential::{CredentialError, client_builder};

const SESSION_MINUTES: u32 = 60;
const RENEW_BEFORE_END: Duration = Duration::from_secs(120);

/// A client whose HTTPS requests go through a crownos-secretsd proxy that adds the key.
#[derive(Clone)]
pub struct ProxiedClient {
    pub client: reqwest::Client,
    pub renew_at: Instant,
}

impl ProxiedClient {
    pub fn is_fresh(&self) -> bool {
        Instant::now() < self.renew_at
    }
}

pub async fn open_proxy_session(secret: &str) -> Result<ProxiedClient, CredentialError> {
    let name = SecretName::parse(secret)
        .map_err(|error| CredentialError::Secret(format!("`{secret}`: {error}")))?;
    let mut daemon = AsyncClient::connect(secrets::SERVICE).map_err(|error| {
        CredentialError::Secret(format!("crownos-secretsd is not running: {error}"))
    })?;
    let reply = daemon
        .call::<secrets::proxy_session>(
            &secrets::proxy_session {
                names: vec![name],
                ttl_minutes: SESSION_MINUTES,
            },
            Vec::new(),
        )
        .await
        .map_err(|error| CredentialError::Secret(error.to_string()))?;
    match reply {
        ProxySessionReply::Started(session) => proxied_client(&session).await,
        ProxySessionReply::Refused(outcome) => Err(CredentialError::Secret(format!(
            "crownos-secretsd refused `{secret}`: {outcome:?}"
        ))),
        ProxySessionReply::NoHttpBinding(_) => Err(CredentialError::Secret(format!(
            "`{secret}` has no HTTP binding; add one with `crownos-secrets bind {secret} --http-host ...`"
        ))),
    }
}

async fn proxied_client(session: &ProxySession) -> Result<ProxiedClient, CredentialError> {
    let authority = tokio::fs::read(&session.ca_path)
        .await
        .map_err(|error| CredentialError::Secret(format!("read {}: {error}", session.ca_path)))?;
    let authority = reqwest::Certificate::from_pem(&authority)
        .map_err(|error| CredentialError::Client(error.to_string()))?;
    let proxy = reqwest::Proxy::all(&session.proxy_url)
        .map_err(|error| CredentialError::Client(error.to_string()))?;
    let client = client_builder()
        .proxy(proxy)
        .add_root_certificate(authority)
        .build()
        .map_err(|error| CredentialError::Client(error.to_string()))?;
    let lifetime = Duration::from_secs(session.expires_in_seconds).saturating_sub(RENEW_BEFORE_END);
    Ok(ProxiedClient {
        client,
        renew_at: Instant::now() + lifetime,
    })
}
