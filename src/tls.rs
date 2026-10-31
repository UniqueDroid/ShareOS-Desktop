//! Explicit, origin-scoped trust for a self-signed NAS certificate.
//! Inspection sends only an anonymous GET. Login still uses normal validation
//! unless the user explicitly trusts the displayed certificate for this session.
use anyhow::{ensure, Context, Result};
use reqwest::blocking::{Client, ClientBuilder};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    DigitallySignedStruct, SignatureScheme,
};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Certificate {
    pub origin: String,
    pub der: Vec<u8>,
}
impl Certificate {
    pub fn fingerprint(&self) -> String {
        Sha256::digest(&self.der)
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }
}
pub fn inspect(address: &str) -> Result<Certificate> {
    let base = crate::nas::base_url(address)?;
    ensure!(
        base.scheme() == "https",
        "Zertifikate können nur für HTTPS geprüft werden"
    );
    // Never attach a password, session, cookie jar or Authorization header here.
    let response = Client::builder()
        .danger_accept_invalid_certs(true)
        .tls_info(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()?
        .get(base.clone())
        .send()?;
    let der = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|info| info.peer_certificate())
        .context("Server hat kein Zertifikat geliefert")?
        .to_vec();
    Ok(Certificate {
        origin: base.origin().ascii_serialization(),
        der,
    })
}
pub fn configure(
    builder: ClientBuilder,
    address: &url::Url,
    trusted: Option<&Certificate>,
) -> Result<ClientBuilder> {
    let Some(cert) = trusted else {
        return Ok(builder);
    };
    ensure!(
        address.scheme() == "https" && cert.origin == address.origin().ascii_serialization(),
        "Zertifikatsvertrauen gilt nur für die bestätigte Box-Adresse"
    );
    Ok(builder.use_preconfigured_tls(client_config(address, Some(cert))?))
}
pub fn client_config(
    address: &url::Url,
    trusted: Option<&Certificate>,
) -> Result<rustls::ClientConfig> {
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?;
    if let Some(cert) = trusted {
        ensure!(
            address.scheme() == "https" && cert.origin == address.origin().ascii_serialization(),
            "Zertifikat gehört zu einer anderen Box-Adresse"
        );
        Ok(builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCertificate(cert.der.clone())))
            .with_no_client_auth())
    } else {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        Ok(builder.with_root_certificates(roots).with_no_client_auth())
    }
}

#[derive(Debug)]
struct PinnedCertificate(Vec<u8>);
impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // The exact certificate was explicitly approved for this origin.
        // This also supports the NAS's generated certificate without a LAN DNS alias.
        if end_entity.as_ref() == self.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "Das Zertifikat der Box hat sich geändert. Erneut prüfen und bestätigen.".into(),
            ))
        }
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
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
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
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pin_requires_exact_certificate() {
        let verifier = PinnedCertificate(vec![1, 2, 3]);
        let name = ServerName::try_from("box.local").unwrap();
        assert!(verifier
            .verify_server_cert(
                &CertificateDer::from(vec![1, 2, 3]),
                &[],
                &name,
                &[],
                UnixTime::since_unix_epoch(Duration::from_secs(0))
            )
            .is_ok());
        assert!(verifier
            .verify_server_cert(
                &CertificateDer::from(vec![1, 2, 4]),
                &[],
                &name,
                &[],
                UnixTime::since_unix_epoch(Duration::from_secs(0))
            )
            .is_err());
    }
    #[test]
    fn trust_cannot_be_reused_for_another_origin() {
        let cert = Certificate {
            origin: "https://box.local".into(),
            der: vec![1, 2, 3],
        };
        for address in [
            "https://other.local",
            "http://box.local",
            "https://box.local:8443",
        ] {
            assert!(configure(
                Client::builder(),
                &crate::nas::base_url(address).unwrap(),
                Some(&cert)
            )
            .is_err());
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use rustls::pki_types::PrivatePkcs8KeyDer;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    // A real TLS server, including a deliberately different certificate hostname.
    // No NAS, DNS entry or external server is involved.
    pub(crate) fn server(connections: usize) -> (String, thread::JoinHandle<Vec<String>>) {
        let key = rcgen::generate_simple_self_signed(vec!["box.local".into()]).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![key.cert.der().clone()],
            PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("https://{}/", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..connections {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let connection = rustls::ServerConnection::new(Arc::new(config.clone())).unwrap();
                let mut stream = rustls::StreamOwned::new(connection, socket);
                let mut request = Vec::new();
                loop {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        let header = String::from_utf8(request.clone()).unwrap();
                        let size = header
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        let mut body = vec![0; size];
                        stream.read_exact(&mut body).unwrap();
                        request.extend(body);
                        requests.push(String::from_utf8(request).unwrap());
                        let body = if requests.last().unwrap().starts_with("POST /ubus ") {
                            r#"{"result":[0,{"ubus_rpc_session":"test-session","role":"user"}]}"#
                        } else {
                            "shareos.files shareos.me"
                        };
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .unwrap();
                        stream.flush().unwrap();
                        break;
                    }
                }
            }
            requests
        });
        (address, handle)
    }
    #[test]
    fn self_signed_tls_requires_approval_and_rejects_a_different_pin() {
        let (address, server) = server(4);
        let base = crate::nas::base_url(&address).unwrap();
        assert!(
            crate::nas::Nas::connect(&address, "alice".into(), "must-not-be-sent", None).is_err()
        );
        let cert = inspect(&address).unwrap();
        assert_eq!(cert.fingerprint().len(), 95);
        let client = configure(Client::builder().no_proxy(), &base, Some(&cert))
            .unwrap()
            .build()
            .unwrap();
        assert!(client
            .get(base.clone())
            .send()
            .unwrap()
            .status()
            .is_success());
        let mut wrong = cert.clone();
        wrong.der.push(0);
        let client = configure(Client::builder().no_proxy(), &base, Some(&wrong))
            .unwrap()
            .build()
            .unwrap();
        assert!(client.get(base).send().is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2); // refused TLS connections sent no HTTP request
        assert!(requests
            .iter()
            .all(|request| request.starts_with("GET / HTTP/1.1")
                && !request.to_lowercase().contains("authorization:")
                && !request.contains("password")));
    }
    #[test]
    fn approved_certificate_allows_login_and_user_binding_over_tls() {
        let (address, server) = server(3);
        let cert = inspect(&address).unwrap();
        let nas = crate::nas::Nas::connect(&address, "alice".into(), "test-password", Some(&cert))
            .unwrap();
        assert!(!nas.admin);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET / HTTP/1.1"));
        assert!(!requests[0].contains("test-password"));
        let calls: Vec<serde_json::Value> = requests[1..]
            .iter()
            .map(|request| serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap())
            .collect();
        assert_eq!(calls[0]["params"][1], "session");
        assert_eq!(calls[1]["params"][1], "shareos.me");
        assert_eq!(calls[1]["params"][2], "bind");
    }
    #[test]
    fn discovery_identifies_https_box_with_self_signed_certificate() {
        let (address, server) = server(1);
        let client = Client::builder()
            .no_proxy()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        assert!(crate::nas::probe(&client, &address).unwrap());
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /app.js HTTP/1.1"));
    }
}
