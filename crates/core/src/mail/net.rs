//! Connections to mail servers: TLS, STARTTLS, or plain for servers on this computer
//! only. Certificates are checked against the operating system's trust store, except for
//! servers on this computer (Proton Bridge uses its own self-signed certificate; the
//! traffic never leaves the machine).

use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_imap::{Client, Session};
use mimi_protocol::MailSecurity;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::MailError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// A connection to a mail server, encrypted or (loopback only) not.
#[derive(Debug)]
pub enum MailStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for MailStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MailStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            MailStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for MailStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MailStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            MailStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MailStream::Plain(s) => Pin::new(s).poll_flush(cx),
            MailStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            MailStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            MailStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

pub type ImapSession = Session<MailStream>;

/// Whether `host` is this computer.
pub fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Plain connections are only for servers on this computer: anything else would send
/// the password and the mail in the clear.
pub fn check_security(host: &str, security: MailSecurity) -> Result<(), MailError> {
    if security == MailSecurity::Plain && !is_loopback(host) {
        return Err(MailError::Refused(
            "Unencrypted connections are only allowed to servers on this computer.".to_owned(),
        ));
    }
    Ok(())
}

pub fn tls_config(host: &str) -> Result<Arc<rustls::ClientConfig>, MailError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| MailError::Tls(e.to_string()))?
        .dangerous();
    let verifier: Arc<dyn ServerCertVerifier> = if is_loopback(host) {
        Arc::new(LoopbackVerifier(provider))
    } else {
        Arc::new(
            rustls_platform_verifier::Verifier::new(provider)
                .map_err(|e| MailError::Tls(e.to_string()))?,
        )
    };
    Ok(Arc::new(
        builder
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth(),
    ))
}

async fn tcp(host: &str, port: u16) -> Result<TcpStream, MailError> {
    tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| MailError::Unreachable(host.to_owned()))?
        .map_err(|_| MailError::Unreachable(host.to_owned()))
}

async fn tls(host: &str, stream: TcpStream) -> Result<MailStream, MailError> {
    let name = ServerName::try_from(host.to_owned()).map_err(|e| MailError::Tls(e.to_string()))?;
    let tls = TlsConnector::from(tls_config(host)?)
        .connect(name, stream)
        .await
        .map_err(|e| MailError::Tls(e.to_string()))?;
    Ok(MailStream::Tls(Box::new(tls)))
}

/// Opens an IMAP session and logs in.
pub async fn imap_login(
    host: &str,
    port: u16,
    security: MailSecurity,
    username: &str,
    password: &str,
) -> Result<ImapSession, MailError> {
    check_security(host, security)?;
    let stream = tcp(host, port).await?;
    let mut client = match security {
        MailSecurity::Tls => Client::new(tls(host, stream).await?),
        MailSecurity::Plain => Client::new(MailStream::Plain(stream)),
        MailSecurity::StartTls => {
            let mut plain = Client::new(MailStream::Plain(stream));
            greeting(&mut plain).await?;
            plain
                .run_command_and_check_ok("STARTTLS", None)
                .await
                .map_err(|e| MailError::Tls(e.to_string()))?;
            let MailStream::Plain(tcp) = plain.into_inner() else {
                unreachable!("the stream was plain")
            };
            Client::new(tls(host, tcp).await?)
        }
    };
    if security != MailSecurity::StartTls {
        greeting(&mut client).await?;
    }
    tokio::time::timeout(CONNECT_TIMEOUT, client.login(username, password))
        .await
        .map_err(|_| MailError::Unreachable(host.to_owned()))?
        .map_err(|(e, _)| match e {
            async_imap::error::Error::No(_) | async_imap::error::Error::Bad(_) => MailError::Login,
            other => MailError::Protocol(other.to_string()),
        })
}

async fn greeting(client: &mut Client<MailStream>) -> Result<(), MailError> {
    tokio::time::timeout(CONNECT_TIMEOUT, client.read_response())
        .await
        .map_err(|_| MailError::Protocol("the server didn't greet us".to_owned()))?
        .map_err(|e| MailError::Protocol(e.to_string()))?
        .ok_or_else(|| MailError::Protocol("the server closed the connection".to_owned()))?;
    Ok(())
}

/// Accepts any certificate, for servers on this computer only (see the module docs).
#[derive(Debug)]
struct LoopbackVerifier(Arc<CryptoProvider>);

impl ServerCertVerifier for LoopbackVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_only_on_this_computer() {
        assert!(check_security("127.0.0.1", MailSecurity::Plain).is_ok());
        assert!(check_security("localhost", MailSecurity::Plain).is_ok());
        assert!(check_security("[::1]", MailSecurity::Plain).is_ok());
        assert!(check_security("imap.mail.me.com", MailSecurity::Plain).is_err());
        assert!(check_security("192.168.1.2", MailSecurity::Plain).is_err());
        assert!(check_security("imap.mail.me.com", MailSecurity::Tls).is_ok());
    }
}
