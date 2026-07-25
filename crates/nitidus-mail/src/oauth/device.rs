//! The device-code grant (RFC 8628): request the codes, surface the
//! user prompt, poll the token endpoint until approved, denied, or
//! expired.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use io_oauth::rfc6749::issue_access_token::{
    Oauth20AccessTokenErrorCode, Oauth20AccessTokenResponse,
};
use io_oauth::rfc8628::auth::{
    Oauth20DeviceAuthRequest, Oauth20DeviceAuthRequestParams, Oauth20DeviceAuthRequestResult,
    Oauth20DeviceAuthSuccessParams,
};
use io_oauth::rfc8628::token::{
    Oauth20DeviceAccessTokenRequest, Oauth20DeviceAccessTokenRequestParams,
    Oauth20DeviceAccessTokenRequestResult,
};
use secrecy::SecretString;

use super::grant::{Grant, grant_from};
use super::{PumpStep, Url, connect_endpoint, post_request, pump};
use crate::error::MailError;

const SLOW_DOWN_BACKOFF: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct DeviceFlowConfig {
    pub device_endpoint: Url,
    pub token_endpoint: Url,
    pub client_id: String,
    pub scopes: Vec<String>,
}

/// What the user must do to approve a device grant.
#[derive(Clone, Debug)]
pub struct DevicePrompt {
    pub user_code: String,
    pub verification_uri: String,
}

/// Runs the device-code grant: requests the codes, reports the prompt,
/// then polls until approved, denied, or expired.
pub async fn authorize_device(
    config: DeviceFlowConfig,
    on_prompt: impl FnOnce(DevicePrompt) + Send,
) -> Result<Grant, MailError> {
    let approved = request_device_codes(&config).await?;
    on_prompt(DevicePrompt {
        user_code: approved.user_code.clone(),
        verification_uri: approved.verification_uri.clone(),
    });
    let deadline = Instant::now() + Duration::from_secs(approved.expires_in as u64);
    let mut interval = Duration::from_secs(approved.interval as u64);
    loop {
        tokio::time::sleep(interval).await;
        if Instant::now() >= deadline {
            return Err(MailError::Backend(
                "device authorization expired before approval".to_owned(),
            ));
        }
        match poll_device_token(&config, &approved.device_code).await? {
            Ok(success) => return grant_from(Ok(success)),
            Err(denied) => match denied.error {
                Oauth20AccessTokenErrorCode::AuthorizationPending => {}
                Oauth20AccessTokenErrorCode::SlowDown => interval += SLOW_DOWN_BACKOFF,
                _ => {
                    return Err(MailError::Backend(format!(
                        "device authorization denied: {:?}",
                        denied.error
                    )));
                }
            },
        }
    }
}

async fn request_device_codes(
    config: &DeviceFlowConfig,
) -> Result<Oauth20DeviceAuthSuccessParams, MailError> {
    let params = Oauth20DeviceAuthRequestParams {
        client_id: Cow::Borrowed(&config.client_id),
        scope: config
            .scopes
            .iter()
            .map(|s| Cow::Borrowed(s.as_str()))
            .collect(),
    };
    let request = post_request(&config.device_endpoint);
    let mut stream = connect_endpoint(&config.device_endpoint).await?;
    let mut coroutine = Oauth20DeviceAuthRequest::new(request, params);
    pump(&mut stream, |arg| {
        Ok(match coroutine.resume(arg) {
            Oauth20DeviceAuthRequestResult::Ok(response) => PumpStep::Done(response),
            Oauth20DeviceAuthRequestResult::WantsRead => PumpStep::Read,
            Oauth20DeviceAuthRequestResult::WantsWrite(bytes) => PumpStep::Write(bytes),
            Oauth20DeviceAuthRequestResult::Err(error) => {
                return Err(MailError::Backend(format!("device authorization: {error}")));
            }
        })
    })
    .await?
    .map_err(|denied| MailError::Backend(format!("device authorization: {:?}", denied.error)))
}

async fn poll_device_token(
    config: &DeviceFlowConfig,
    device_code: &SecretString,
) -> Result<Oauth20AccessTokenResponse, MailError> {
    let params = Oauth20DeviceAccessTokenRequestParams {
        client_id: Cow::Borrowed(&config.client_id),
        device_code: device_code.clone(),
    };
    let request = post_request(&config.token_endpoint);
    let mut stream = connect_endpoint(&config.token_endpoint).await?;
    let mut coroutine = Oauth20DeviceAccessTokenRequest::new(request, params);
    pump(&mut stream, |arg| {
        Ok(match coroutine.resume(arg) {
            Oauth20DeviceAccessTokenRequestResult::Ok(response) => PumpStep::Done(response),
            Oauth20DeviceAccessTokenRequestResult::WantsRead => PumpStep::Read,
            Oauth20DeviceAccessTokenRequestResult::WantsWrite(bytes) => PumpStep::Write(bytes),
            Oauth20DeviceAccessTokenRequestResult::Err(error) => {
                return Err(MailError::Backend(format!("device poll: {error}")));
            }
        })
    })
    .await
}
