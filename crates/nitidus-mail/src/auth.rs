//! How a connection proves itself: plain LOGIN/AUTH PLAIN with a
//! password, or SASL XOAUTH2 with a bearer token served by a
//! [`TokenRefresher`].

use std::sync::Arc;

use secrecy::SecretString;

use crate::oauth::TokenRefresher;

#[derive(Clone)]
pub enum MailAuth {
    Login(SecretString),
    Xoauth2(Arc<TokenRefresher>),
}
