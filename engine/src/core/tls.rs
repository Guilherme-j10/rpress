use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls_pemfile::{certs, private_key};
use tokio_rustls::TlsAcceptor;

/// TLS configuration for the Rpress server, backed by rustls.
pub struct RpressTlsConfig {
    pub(crate) acceptor: TlsAcceptor,
}

impl RpressTlsConfig {
    /// Loads a TLS certificate chain and private key from PEM files.
    pub fn from_pem(cert_path: &str, key_path: &str) -> anyhow::Result<Self> {
        let cert_file = File::open(cert_path)
            .map_err(|e| anyhow::anyhow!("Failed to open cert file '{}': {}", cert_path, e))?;
        let key_file = File::open(key_path)
            .map_err(|e| anyhow::anyhow!("Failed to open key file '{}': {}", key_path, e))?;

        let cert_chain: Vec<_> = certs(&mut BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to parse certificates: {}", e))?;

        let key = private_key(&mut BufReader::new(key_file))
            .map_err(|e| anyhow::anyhow!("Failed to parse private key: {}", e))?
            .ok_or_else(|| anyhow::anyhow!("No private key found in '{}'", key_path))?;

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)
            .map_err(|e| anyhow::anyhow!("Invalid TLS configuration: {}", e))?;

        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
        })
    }

    /// Creates a TLS configuration from an existing rustls `ServerConfig`.
    pub fn from_config(mut config: ServerConfig) -> Self {
        if config.alpn_protocols.is_empty() {
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        }

        Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
        }
    }
}
