use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use suja_model::ModelError;
use tokio::sync::Mutex;

use crate::secret_proxy::{ProxiedClient, open_proxy_session};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a backend gets the right to call its server. The config names a key; it never holds one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Credential {
    #[default]
    None,
    /// The key is in this environment variable of sujad.
    Environment { variable: String },
    /// crownos-secretsd adds the key in its HTTPS proxy; sujad never sees it.
    Secret { name: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("the API key in ${0} is not set")]
    MissingEnvironment(String),
    #[error("secret {0}")]
    Secret(String),
    #[error("HTTP client: {0}")]
    Client(String),
}

impl From<CredentialError> for ModelError {
    fn from(error: CredentialError) -> Self {
        Self::Unavailable(error.to_string())
    }
}

/// A client for one request, and the API key when the backend must add it itself.
#[derive(Clone)]
pub struct Authorized {
    pub client: reqwest::Client,
    pub api_key: Option<Arc<str>>,
}

pub struct HttpCredentials {
    credential: Credential,
    client: reqwest::Client,
    proxied: Mutex<Option<ProxiedClient>>,
}

pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(concat!("suja/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
}

impl HttpCredentials {
    pub fn new(credential: Credential) -> Result<Self, CredentialError> {
        let client = client_builder()
            .build()
            .map_err(|error| CredentialError::Client(error.to_string()))?;
        Ok(Self {
            credential,
            client,
            proxied: Mutex::new(None),
        })
    }

    pub async fn authorized(&self) -> Result<Authorized, CredentialError> {
        match &self.credential {
            Credential::None => Ok(self.direct(None)),
            Credential::Environment { variable } => std::env::var(variable)
                .map(|key| self.direct(Some(key.into())))
                .map_err(|_| CredentialError::MissingEnvironment(variable.clone())),
            Credential::Secret { name } => {
                let mut proxied = self.proxied.lock().await;
                let client = match proxied.as_ref().filter(|session| session.is_fresh()) {
                    Some(session) => session.client.clone(),
                    None => {
                        let session = open_proxy_session(name).await?;
                        let client = session.client.clone();
                        *proxied = Some(session);
                        client
                    }
                };
                Ok(Authorized {
                    client,
                    api_key: None,
                })
            }
        }
    }

    fn direct(&self, api_key: Option<Arc<str>>) -> Authorized {
        Authorized {
            client: self.client.clone(),
            api_key,
        }
    }
}
