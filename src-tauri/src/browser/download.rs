use crate::browser::{validate_url, BrowserError};
use futures_util::StreamExt;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub async fn download_url_to_path(url: &str, path: &Path) -> Result<(), BrowserError> {
    let url = validate_url(url)?;
    if url == "about:blank" {
        return Err(BrowserError::DownloadFailed(
            "about:blank cannot be downloaded".into(),
        ));
    }

    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
    if !response.status().is_success() {
        return Err(BrowserError::DownloadFailed(format!(
            "server returned HTTP {}",
            response.status()
        )));
    }

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
    }
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
    }
    file.flush()
        .await
        .map_err(|error| BrowserError::DownloadFailed(error.to_string()))?;
    Ok(())
}
