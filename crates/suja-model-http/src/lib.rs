mod credential;
#[cfg(feature = "fake-server")]
pub mod fake_server;
mod response;
mod sse;

pub use credential::{Authorized, Credential, CredentialError, HttpCredentials};
pub use response::{for_each_event, send_request};
pub use sse::{ServerSentEvent, SseParser};
