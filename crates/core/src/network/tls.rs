use anyhow::Result;
use rustls::{Certificate, ClientConfig, OwnedTrustAnchor, RootCertStore};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, TlsConnector};

#[derive(Debug, Clone)]
pub struct TlsInfo {
    pub protocol: Option<String>,
}

/// Create a TLS connector with system and bundled Mozilla root certificates
pub fn create_tls_connector() -> Result<TlsConnector> {
    create_tls_connector_with_extra_roots(std::iter::empty::<Certificate>())
}

pub fn create_tls_connector_with_extra_roots<I>(extra_roots: I) -> Result<TlsConnector>
where
    I: IntoIterator<Item = Certificate>,
{
    let native_roots = rustls_native_certs::load_native_certs()?
        .into_iter()
        .map(|cert| Certificate(cert.0));
    let root_store = build_root_cert_store(native_roots, extra_roots)?;

    let config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(TlsConnector::from(Arc::new(config)))
}

fn build_root_cert_store<N, E>(native_roots: N, extra_roots: E) -> Result<RootCertStore>
where
    N: IntoIterator<Item = Certificate>,
    E: IntoIterator<Item = Certificate>,
{
    let mut root_store = RootCertStore::empty();
    for cert in native_roots {
        root_store.add(&cert)?;
    }

    // Bundled roots cover public CAs missing from an OS store while preserving local roots.
    root_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|anchor| {
        OwnedTrustAnchor::from_subject_spki_name_constraints(
            anchor.subject,
            anchor.spki,
            anchor.name_constraints,
        )
    }));

    for cert in extra_roots {
        root_store.add(&cert)?;
    }
    Ok(root_store)
}

/// Upgrade a TCP stream to TLS
pub async fn upgrade_to_tls(
    stream: TcpStream,
    domain: &str,
) -> Result<(TlsStream<TcpStream>, TlsInfo)> {
    let connector = create_tls_connector()?;
    let domain = match domain.parse::<std::net::IpAddr>() {
        Ok(ip) => rustls::ServerName::IpAddress(ip),
        Err(_) => rustls::ServerName::try_from(domain)?,
    };
    let tls_stream = connector.connect(domain, stream).await?;
    let protocol = tls_stream
        .get_ref()
        .1
        .protocol_version()
        .map(|version| match version {
            rustls::ProtocolVersion::TLSv1_2 => "TLSv1.2".to_string(),
            rustls::ProtocolVersion::TLSv1_3 => "TLSv1.3".to_string(),
            other => format!("{:?}", other),
        });
    Ok((tls_stream, TlsInfo { protocol }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_roots_include_gts_without_native_roots() {
        let roots = build_root_cert_store(
            std::iter::empty::<Certificate>(),
            std::iter::empty::<Certificate>(),
        )
        .unwrap();

        assert_eq!(roots.len(), webpki_roots::TLS_SERVER_ROOTS.len());
        let gts_root = webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .find(|anchor| {
                anchor
                    .subject
                    .windows(b"GTS Root R1".len())
                    .any(|window| window == b"GTS Root R1")
            })
            .expect("Mozilla roots should include GTS Root R1");
        assert!(roots.roots.iter().any(|root| {
            let subject: &[u8] = root.subject().as_ref();
            subject.ends_with(gts_root.subject)
        }));
    }
}
