//! Bounded HTTP fetching on a shared runtime, independently of GPUI's asset cache.

use super::decode::MAX_ENCODED_BYTES;
use futures::StreamExt;
use std::{
    future::Future,
    pin::Pin,
    sync::OnceLock,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{runtime::Runtime, task::JoinHandle};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const IO_TIMEOUT: Duration = Duration::from_secs(5);

struct Client {
    runtime: Runtime,
    http: reqwest::Client,
}

impl Client {
    fn shared() -> Result<&'static Self, String> {
        static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
        CLIENT
            .get_or_init(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .thread_name("opalglass-images-http")
                    .enable_all()
                    .build()
                    .map_err(|_| "Could not start image loader".to_string())?;
                let http = reqwest::Client::builder()
                    .use_rustls_tls()
                    .connect_timeout(IO_TIMEOUT)
                    .read_timeout(IO_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT)
                    .redirect_policy(reqwest::redirect::Policy::limited(5))
                    .pool_max_idle_per_host(2)
                    .pool_idle_timeout(Duration::from_secs(30))
                    .user_agent(concat!("Opalglass/", env!("CARGO_PKG_VERSION")))
                    .build()
                    .map_err(|_| "Could not initialize image requests".to_string())?;
                Ok(Self { runtime, http })
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// Fetch an HTTP(S) image without retaining its encoded bytes in another cache.
///
/// Poll this future on a background executor: the first poll initializes the
/// shared HTTP runtime and TLS client. All network operations run on that
/// runtime. Dropping the future cancels its request, including before headers.
pub async fn fetch(url: String) -> Result<Vec<u8>, String> {
    let url = normalized_url(&url)
        .and_then(|url| reqwest::Url::parse(&url).ok())
        .ok_or_else(|| "Invalid remote image URL".to_string())?;
    let client = Client::shared()?;
    let http = client.http.clone();
    AbortOnDrop(
        client
            .runtime
            .spawn(async move { fetch_inner(http, url).await }),
    )
    .await
    .map_err(|_| "Image request was interrupted".to_string())?
}

/// Bound parsing work and share fragment variants of the same downloaded image.
pub fn normalized_url(raw: &str) -> Option<String> {
    if raw.len() > 8192 {
        return None;
    }
    let mut url = reqwest::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    Some(url.into())
}

async fn fetch_inner(http: reqwest::Client, url: reqwest::Url) -> Result<Vec<u8>, String> {
    let response = http.get(url).send().await.map_err(request_error)?;
    if !response.status().is_success() {
        return Err(format!(
            "Image server returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let length = response.content_length();
    if length.is_some_and(|length| length > MAX_ENCODED_BYTES as u64) {
        return Err("Remote image exceeds the download limit".to_string());
    }
    let mut bytes = Vec::with_capacity(length.unwrap_or(0) as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(request_error)?;
        if chunk.len() > MAX_ENCODED_BYTES - bytes.len() {
            return Err("Remote image exceeds the download limit".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn request_error(error: reqwest::Error) -> String {
    // URL queries can contain credentials. Keep transport diagnostics useful
    // without including the request URL or the server's arbitrary response body.
    if error.is_timeout() {
        "Image request timed out"
    } else if error.is_redirect() {
        "Image server redirected too many times"
    } else if error.is_connect() {
        "Could not connect to the image server"
    } else {
        "Image download failed"
    }
    .to_string()
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Future for AbortOnDrop<T> {
    type Output = Result<T, tokio::task::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
#[path = "http/tests.rs"]
mod tests;
