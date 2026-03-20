use futures_util::StreamExt;
use parking_lot::RwLock;
use reqwest::{Client, IntoUrl};
use serde::Serialize;
use std::{fs::File as StdFile, io::Write, sync::Arc, time};
use tempfile::tempfile;
use thiserror::Error;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{
        Semaphore,
        mpsc::{self, Sender},
    },
    time::sleep,
};
use url::Url;

pub struct Downloader<F: Fn(DownloaderState)> {
    inner: RwLock<DownloaderInner>,
    client: Client,
    url: Arc<Url>,
    event_callback: Option<F>,
}

impl<F: Fn(DownloaderState)> std::fmt::Debug for Downloader<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read();
        f.debug_struct("Downloader")
            .field("inner", &inner)
            .field("client", &self.client)
            .field("url", &self.url)
            .finish()
    }
}

#[derive(Debug)]
struct DownloaderInner {
    state: DownloaderState,
    file: Option<File>,
    total_size: u64,
    semaphore: Arc<Semaphore>,
    chunks: Vec<Arc<RwLock<ChunkThread>>>,
}

impl Default for DownloaderInner {
    fn default() -> Self {
        Self {
            state: DownloaderState::default(),
            file: None,
            total_size: 0,
            semaphore: Arc::new(Semaphore::new(1)),
            chunks: Vec::new(),
        }
    }
}

pub struct DownloaderBuilder<F: Fn(DownloaderState)> {
    client: Option<Client>,
    url: Option<Url>,
    file: Option<File>,
    event_callback: Option<F>,
}

impl<F: Fn(DownloaderState)> DownloaderBuilder<F> {
    pub fn new() -> Self {
        Self {
            client: None,
            url: None,
            file: None,
            event_callback: None,
        }
    }

    pub fn set_client(mut self, client: Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn set_url<U: IntoUrl>(mut self, url: U) -> Result<Self, DownloaderError> {
        self.url = Some(url.into_url()?);
        Ok(self)
    }

    pub fn set_file(mut self, file: File) -> Self {
        self.file = Some(file);
        self
    }

    pub fn set_event_callback(mut self, callback: F) -> Self {
        self.event_callback = Some(callback);
        self
    }

    pub fn build(self) -> Result<Downloader<F>, DownloaderError> {
        let client = self.client.unwrap_or_default();
        let url = self
            .url
            .ok_or(DownloaderError::Other("URL is not set".to_string()))?;
        let nums = num_cpus::get();
        Ok(Downloader {
            inner: RwLock::new(DownloaderInner {
                file: self.file,
                semaphore: Arc::new(Semaphore::new(nums)),
                chunks: Vec::with_capacity(nums),
                ..Default::default()
            }),
            event_callback: self.event_callback,
            client,
            url: Arc::new(url),
        })
    }
}

#[derive(Debug, Serialize, Default, Clone, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DownloaderState {
    #[default]
    Idle,
    Downloading,
    WaitingForMerge,
    Merging,
    Failed(String),
    Finished,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct DownloadStatus {
    pub state: DownloaderState,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub chunks: Vec<ChunkStatus>,
    pub now: u64,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct ChunkStatus {
    pub state: ChunkThreadState,
    pub start: usize,
    pub end: usize,
    pub downloaded: usize,
    pub speed: f64,
}

enum ChunkThreadEvent {
    DecreaseSemaphore(DecreaseSemaphoreReason),
    Finish,
}

enum DecreaseSemaphoreReason {
    Cause(anyhow::Error),
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ChunkThreadState {
    Idle,
    Downloading,
    Finished,
}

#[derive(Debug)]
struct ChunkThread {
    client: Client,
    sender: Sender<ChunkThreadEvent>,
    semaphore: Arc<Semaphore>,
    file: StdFile,
    url: Arc<Url>,
    state: ChunkThreadState,
    start: usize,
    end: usize,
    downloaded: usize,
    speed: f64,
}

#[derive(Error, Debug)]
pub enum DownloaderError {
    #[error("Failed to perform a request, reason: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("Failed to download file, reason: {0}")]
    DownloadFailed(#[from] anyhow::Error),
    #[error("Failed to write file")]
    WriteFailed(#[from] std::io::Error),
    #[error("Failed to confirm file size")]
    ConfirmSizeFailed,
    #[error("Other error: {0}")]
    Other(String),
}

impl Serialize for DownloaderError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        format!("{self}").serialize(serializer)
    }
}

impl<F: Fn(DownloaderState)> Downloader<F> {
    fn dispatch_event(&self, state: DownloaderState) {
        {
            let mut inner = self.inner.write();
            inner.state = state.clone();
        }
        if let Some(callback) = &self.event_callback {
            callback(state);
        }
    }

    async fn confirm_file_status(&self) -> Result<u64, DownloaderError> {
        let response = self
            .client
            .head(self.url.clone().as_str())
            .send()
            .await?
            .error_for_status()?;
        let headers = response.headers();
        if headers
            .get(reqwest::header::ACCEPT_RANGES)
            .is_some_and(|v| v == "none")
        {
            return Err(DownloaderError::Other(
                "Server does not support RANGE requests".to_string(),
            ));
        }
        let total_size = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if total_size == 0 {
            return Err(DownloaderError::ConfirmSizeFailed);
        }
        {
            let mut inner = self.inner.write();
            inner.total_size = total_size;
        }
        Ok(total_size)
    }

    async fn merge_chunks(&self) -> Result<(), DownloaderError> {
        {
            let inner = self.inner.read();
            if !matches!(inner.state, DownloaderState::WaitingForMerge) {
                return Err(DownloaderError::Other(
                    "Download is not finished".to_string(),
                ));
            }
            if inner.file.is_none() {
                return Err(DownloaderError::Other("File is not set".to_string()));
            }
        }
        self.dispatch_event(DownloaderState::Merging);
        let mut file = {
            let mut inner = self.inner.write();
            inner.file.take().unwrap()
        };
        let chunks = self.get_cloned_chunks();
        for part in &chunks {
            let mut part_file = {
                let part = part.read();
                File::from_std(part.file.try_clone().unwrap())
            };
            part_file.seek(tokio::io::SeekFrom::Start(0)).await?;
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                let read = part_file.read(&mut buffer).await?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read]).await?;
            }
        }
        file.flush().await?;
        self.dispatch_event(DownloaderState::Finished);
        Ok(())
    }

    async fn download(&self) -> Result<(), DownloaderError> {
        let mut total_size = {
            let inner = self.inner.read();
            if inner.file.is_none() {
                return Err(DownloaderError::Other("File is not set".to_string()));
            }
            inner.total_size
        };
        if total_size == 0 {
            total_size = self.confirm_file_status().await?;
        }

        let mut file = {
            let mut inner = self.inner.write();
            inner.file.take().unwrap()
        };
        file.set_len(total_size).await?;
        {
            let mut inner = self.inner.write();
            inner.file = Some(file);
        }

        let counts = {
            let inner = self.inner.read();
            inner.semaphore.available_permits() as u64
        };
        let chunk_size = total_size / counts;
        let (tx, mut rx) = mpsc::channel(10);
        self.dispatch_event(DownloaderState::Downloading);

        for chunk in 0..counts {
            let start = (chunk * chunk_size) as usize;
            let end = if chunk == counts - 1 {
                total_size as usize
            } else {
                start + (chunk_size as usize) - 1
            };
            let thread = {
                let inner = self.inner.read();
                Arc::new(RwLock::new(ChunkThread::try_new(
                    self.client.clone(),
                    tx.clone(),
                    inner.semaphore.clone(),
                    start,
                    end,
                    self.url.clone(),
                )?))
            };
            let thread_clone = thread.clone();
            tokio::spawn(async move {
                thread_clone.start().await;
            });
            {
                let mut inner = self.inner.write();
                inner.chunks.push(thread);
            }
        }

        let mut downloaded = 0;
        let mut total_permits = counts;
        while let Some(event) = rx.recv().await {
            match event {
                ChunkThreadEvent::Finish => {
                    downloaded += 1;
                    if downloaded == counts {
                        {
                            let inner = self.inner.read();
                            inner.semaphore.close();
                        }
                        self.dispatch_event(DownloaderState::WaitingForMerge);
                        break;
                    }
                }
                ChunkThreadEvent::DecreaseSemaphore(reason) => {
                    total_permits -= 1;
                    if total_permits == 0 {
                        {
                            let inner = self.inner.read();
                            inner.semaphore.close();
                        }
                        match reason {
                            DecreaseSemaphoreReason::Cause(err) => {
                                return Err(DownloaderError::DownloadFailed(err));
                            }
                        }
                    }
                }
            }
        }

        self.merge_chunks().await?;
        Ok(())
    }

    pub async fn start(&self) -> Result<(), DownloaderError> {
        let result = self.download().await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                self.dispatch_event(DownloaderState::Failed(format!("{err}")));
                Err(err)
            }
        }
    }

    fn get_cloned_chunks(&self) -> Vec<Arc<RwLock<ChunkThread>>> {
        let inner = self.inner.read();
        inner.chunks.to_vec()
    }

    pub fn get_current_status(&self) -> DownloadStatus {
        let inner = self.inner.read();
        let total = inner.total_size;
        let mut downloaded = 0;
        let mut speed = 0.0;
        let mut chunks = Vec::with_capacity(inner.chunks.len());
        for chunk in &inner.chunks {
            let chunk = chunk.read();
            downloaded += chunk.downloaded as u64;
            speed += chunk.speed;
            chunks.push(ChunkStatus {
                state: chunk.state.clone(),
                start: chunk.start,
                end: chunk.end,
                downloaded: chunk.downloaded,
                speed: chunk.speed,
            });
        }
        DownloadStatus {
            state: inner.state.clone(),
            downloaded,
            total,
            speed,
            chunks,
            now: chrono::Utc::now().timestamp() as u64,
        }
    }
}

#[async_trait::async_trait]
trait SafeChunkThread {
    fn dispatch_event(&self, state: ChunkThreadState);
    async fn download_chunk(&self) -> Result<(), anyhow::Error>;
    async fn start(&self);
}

impl ChunkThread {
    fn try_new(
        client: Client,
        sender: Sender<ChunkThreadEvent>,
        semaphore: Arc<Semaphore>,
        start: usize,
        end: usize,
        url: Arc<Url>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            client,
            sender,
            semaphore,
            file: tempfile()?,
            url,
            state: ChunkThreadState::Idle,
            start,
            end,
            downloaded: 0,
            speed: 0.0,
        })
    }
}

#[async_trait::async_trait]
impl SafeChunkThread for RwLock<ChunkThread> {
    fn dispatch_event(&self, state: ChunkThreadState) {
        let mut thread = self.write();
        if matches!(state, ChunkThreadState::Finished) {
            thread.speed = 0.0;
        }
        thread.state = state;
    }

    async fn download_chunk(&self) -> Result<(), anyhow::Error> {
        self.dispatch_event(ChunkThreadState::Downloading);
        let response = {
            let (client, url, start, end) = {
                let thread = self.read();
                (
                    thread.client.clone(),
                    thread.url.clone(),
                    thread.start,
                    thread.end,
                )
            };
            client
                .get(url.as_str())
                .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                .send()
                .await?
                .error_for_status()?
        };
        let mut stream = response.bytes_stream();
        let mut tick = time::Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            {
                let mut thread = self.write();
                let elapsed = tick.elapsed().as_secs_f64();
                thread.speed = if elapsed > 0.0 {
                    chunk.len() as f64 / elapsed
                } else {
                    0.0
                };
                thread.file.write_all(&chunk)?;
                thread.downloaded += chunk.len();
            }
            tick = time::Instant::now();
        }
        {
            let mut thread = self.write();
            thread.file.flush()?;
        }
        Ok(())
    }

    async fn start(&self) {
        let semaphore = {
            let thread = self.read();
            thread.semaphore.clone()
        };
        let mut attempts = 0;
        loop {
            let result = {
                let _permit = match semaphore.acquire().await {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                self.download_chunk().await
            };
            match result {
                Ok(_) => {
                    self.dispatch_event(ChunkThreadState::Finished);
                    let sender = {
                        let thread = self.read();
                        thread.sender.clone()
                    };
                    let _ = sender.send(ChunkThreadEvent::Finish).await;
                    break;
                }
                Err(_) if attempts < 3 => {
                    attempts += 1;
                    self.dispatch_event(ChunkThreadState::Idle);
                    sleep(time::Duration::from_secs(1)).await;
                }
                Err(err) => {
                    self.dispatch_event(ChunkThreadState::Idle);
                    let sender = {
                        let thread = self.read();
                        thread.sender.clone()
                    };
                    let _ = sender
                        .send(ChunkThreadEvent::DecreaseSemaphore(
                            DecreaseSemaphoreReason::Cause(err),
                        ))
                        .await;
                    semaphore.forget_permits(1);
                    attempts = 0;
                }
            }
        }
    }
}
