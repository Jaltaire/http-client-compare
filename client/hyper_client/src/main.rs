use std::error::Error;
use std::io::Read;
use std::sync::Arc;
use futures::future::join_all;
use std::time::Duration;
use flate2::read::GzDecoder;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::{header, Request};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::sync::Semaphore;
use tokio::time::Instant;

#[derive(Debug)]
struct NoVerify;

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
        ]
    }
}

struct ClientWrapper {
    client: Client<HttpsConnector<HttpConnector>, Empty<Bytes>>,
    semaphore: Semaphore,
}

impl ClientWrapper {
    fn new(max_concurrent: usize) -> Self {
        let mut http_connector = HttpConnector::new();
        http_connector.set_keepalive(Some(Duration::from_secs(90)));
        http_connector.set_nodelay(true);
        http_connector.enforce_http(false);

        let https_connector = HttpsConnectorBuilder::new()
            .with_tls_config({
                ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(NoVerify))
                    .with_no_client_auth()
            })
            .https_or_http()
            .enable_http1()
            .build();

        Self {
            client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .pool_max_idle_per_host(max_concurrent)
                .pool_idle_timeout(Duration::from_secs(90))
                .set_host(true)
                .build(https_connector),
            semaphore: Semaphore::new(max_concurrent),
        }
    }

    async fn get(&self, url: String) -> Result<(), Box<dyn Error + Send + Sync>> {
        let _permit = self.semaphore.acquire().await;
        loop {
            let start = std::time::Instant::now();
            let req = Request::builder()
                .uri(url.clone())
                .header(header::ACCEPT, "*/*")
                .header(header::ACCEPT_ENCODING, "gzip")
                .header(header::USER_AGENT, "hyper_client")
                .body(Empty::<Bytes>::new())?;
            let res = self.client.request(req).await;
            match res {
                Ok(response) => {
                    let status = response.status();
                    let _body = if response.headers().get(header::CONTENT_ENCODING)
                        == Some(&header::HeaderValue::from_static("gzip"))
                    {
                        let body_bytes = response.collect().await?.to_bytes();
                        let mut decoder = GzDecoder::new(&body_bytes[..]);
                        let mut decompressed = Vec::new();
                        decoder.read_to_end(&mut decompressed)?;
                        decompressed
                    } else {
                        response.collect().await?.to_bytes().to_vec()
                    };
                    println!(
                        "scraped URL (status {}) in {:?}: {}",
                        status,
                        start.elapsed(),
                        url
                    );
                    return Ok(());
                }
                Err(e) => {
                    println!("failed to connect to URL due to {}: {}", e, url)
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let start = Instant::now();

    let client_wrapper = Arc::new(ClientWrapper::new(1000));

    let mut handles = Vec::new();
    for i in 0..1000 {
        let url = format!("https://127.0.0.1:3000/{}", i);
        let handle = tokio::spawn({
            let client_wrapper = client_wrapper.clone();
            async move {
                client_wrapper.get(url).await
            }
        });
        handles.push(handle);
    }
    join_all(handles).await;

    let duration = start.elapsed();
    println!("total execution time: {:?}", duration);
    Ok(())
}
