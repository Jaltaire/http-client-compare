use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use futures::future::join_all;
use reqwest::{header, Client};
use tokio::sync::Semaphore;
use tokio::time::Instant;

struct ClientWrapper {
    client: Client,
    semaphore: Semaphore,
}

impl ClientWrapper {
    fn new(max_concurrent: usize) -> Self {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(max_concurrent)
            .tcp_keepalive(Duration::from_secs(90))
            .gzip(true)
            .build()
            .unwrap();

        Self {
            client,
            semaphore: Semaphore::new(max_concurrent),
        }
    }

    async fn get(&self, url: String) -> Result<(), Box<dyn Error + Send + Sync>> {
        let _permit = self.semaphore.acquire().await;
        loop {
            let start = std::time::Instant::now();
            let res = self
                .client
                .get(&url)
                .header(header::ACCEPT_ENCODING, "gzip")
                .header(header::USER_AGENT, "reqwest_client")
                .send()
                .await;
            match res {
                Ok(response) => {
                    let status = response.status();
                    let _body = response.bytes().await?;
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
