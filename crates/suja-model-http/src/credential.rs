use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use suja_model::ModelError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Where a backend gets the right to call its server. The config names a key; it never holds one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Credential {
    #[default]
    None,
    Environment {
        variable: String,
    },
    Secret {
        name: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("the API key in ${0} is not set")]
    MissingEnvironment(String),
    #[error("the secret `{0}` needs crownos-secretsd, which suja does not use yet")]
    SecretsUnsupported(String),
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
}

impl HttpCredentials {
    pub fn new(credential: Credential) -> Result<Self, CredentialError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("suja/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|error| CredentialError::Client(error.to_string()))?;
        Ok(Self { credential, client })
    }

    pub async fn authorized(&self) -> Result<Authorized, CredentialError> {
        let api_key = match &self.credential {
            Credential::None => None,
            Credential::Environment { variable } => Some(
                std::env::var(variable)
                    .map_err(|_| CredentialError::MissingEnvironment(variable.clone()))?
                    .into(),
            ),
            Credential::Secret { name } => {
                return Err(CredentialError::SecretsUnsupported(name.clone()));
            }
        };
        Ok(Authorized {
            client: self.client.clone(),
            api_key,
        })
    }
}
