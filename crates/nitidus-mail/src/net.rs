//! Shared remote transport: plain TCP or rustls TLS behind one
//! `AsyncRead + AsyncWrite` type, used by the IMAP and SMTP pumps.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::MailError;

pub enum RemoteStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

pub async fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, MailError> {
    TcpStream::connect((host, port))
        .await
        .map_err(|error| MailError::Backend(format!("connect {host}:{port}: {error}")))
}

pub async fn upgrade_tls(tcp: TcpStream, host: &str) -> Result<RemoteStream, MailError> {
    let config = tls_config()?;
    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|error| MailError::Backend(format!("invalid TLS server name {host}: {error}")))?;
    let tls = TlsConnector::from(config)
        .connect(server_name, tcp)
        .await
        .map_err(|error| MailError::Backend(format!("TLS handshake with {host}: {error}")))?;
    Ok(RemoteStream::Tls(Box::new(tls)))
}

fn tls_config() -> Result<Arc<ClientConfig>, MailError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls_platform_verifier::BuilderVerifierExt::with_platform_verifier(
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| MailError::Backend(format!("TLS setup: {error}")))?,
    )
    .map_err(|error| MailError::Backend(format!("TLS verifier: {error}")))?
    .with_no_client_auth();
    Ok(Arc::new(config))
}

impl AsyncRead for RemoteStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RemoteStream::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            RemoteStream::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for RemoteStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            RemoteStream::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            RemoteStream::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RemoteStream::Plain(stream) => Pin::new(stream).poll_flush(cx),
            RemoteStream::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RemoteStream::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            RemoteStream::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}
