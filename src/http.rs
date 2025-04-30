use std::io::{Error, ErrorKind, Result};
use std::{
    ops::{Bound, RangeBounds},
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{StatusCode, header};

fn error_from_status(code: StatusCode) -> Result<StatusCode> {
    if code.is_server_error() {
        Err(Error::other(
            code.canonical_reason().unwrap_or(code.as_str()),
        ))
    } else if code.is_client_error() {
        let kind = match code {
            StatusCode::NOT_FOUND => ErrorKind::NotFound,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ErrorKind::PermissionDenied,
            _ => ErrorKind::Other,
        };
        let msg = code.canonical_reason().unwrap_or(code.as_str());
        Err(Error::new(kind, msg))
    } else {
        Ok(code)
    }
}

struct RangeHeader<R: RangeBounds<u64>>(pub R);

impl<R: RangeBounds<u64>> std::fmt::Display for RangeHeader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bytes=")?;
        match self.0.start_bound() {
            Bound::Unbounded => write!(f, "0-"),
            Bound::Included(v) => write!(f, "{v}-"),
            Bound::Excluded(v) => write!(f, "{}-", v + 1),
        }?;
        match self.0.end_bound() {
            Bound::Unbounded => {}
            Bound::Included(v) => {
                write!(f, "{}", v + 1)?;
            }
            Bound::Excluded(v) => {
                write!(f, "{}", v)?;
            }
        };
        Ok(())
    }
}

struct InnerHttpStore {
    base_url: String,
    client: Arc<reqwest::Client>,
}

pub struct HttpStore(Arc<InnerHttpStore>);

impl HttpStore {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self(Arc::new(InnerHttpStore {
            base_url: base_url.into(),
            client: Arc::new(reqwest::Client::new()),
        }))
    }
}

impl crate::Store for HttpStore {
    type File = HttpStoreFile;

    async fn get_file<P: Into<std::path::PathBuf>>(&self, path: P) -> std::io::Result<Self::File> {
        let path = path.into();
        let relative = crate::util::merge_path(&PathBuf::from("/"), &path)?;
        let url = format!("{}{}", self.0.base_url, relative.to_string_lossy());
        Ok(HttpStoreFile {
            client: self.0.client.clone(),
            url,
        })
    }
}

pub struct HttpStoreFile {
    client: Arc<reqwest::Client>,
    url: String,
}

impl crate::StoreFile for HttpStoreFile {
    type FileReader = HttpStoreFileReader;

    async fn exists(&self) -> std::io::Result<bool> {
        match self.client.head(&self.url).send().await {
            Ok(res) => match res.status() {
                StatusCode::NOT_FOUND => Ok(false),
                other => error_from_status(other).map(|_| true),
            },
            Err(err) => Err(std::io::Error::other(err)),
        }
    }

    async fn read<R: std::ops::RangeBounds<u64>>(
        &self,
        range: R,
    ) -> std::io::Result<Self::FileReader> {
        let res = self
            .client
            .get(&self.url)
            .header(header::RANGE, RangeHeader(range).to_string())
            .send()
            .await
            .map_err(std::io::Error::other)?;
        error_from_status(res.status())?;
        // TODO handle when status code is not 206
        let stream = res.bytes_stream().boxed();
        Ok(HttpStoreFileReader { stream })
    }
}

pub struct HttpStoreFileReader {
    stream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + std::marker::Send>>,
}

impl tokio::io::AsyncRead for HttpStoreFileReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let stream = &mut self.get_mut().stream;

        match Pin::new(stream).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                let len = buf.remaining();
                let to_read = chunk.len().min(len);
                buf.put_slice(&chunk[..to_read]);
                Poll::Ready(Ok(()))
            }
            // Stream has ended with an error, propagate it
            Poll::Ready(Some(Err(err))) => Poll::Ready(Err(Error::new(ErrorKind::Other, err))),
            // No more data to read
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl crate::StoreFileReader for HttpStoreFileReader {}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use tokio::io::AsyncReadExt;

    use crate::{Store, StoreFile, http::HttpStore};

    #[tokio::test]
    async fn should_check_if_file_exists() {
        let mut srv = mockito::Server::new_async().await;
        let mock = srv
            .mock("HEAD", "/not-found.txt")
            .with_status(404)
            .create_async()
            .await;
        let store = HttpStore::new(srv.url());
        let file = store.get_file("/not-found.txt").await.unwrap();
        assert!(!file.exists().await.unwrap());
        mock.assert_async().await;
    }
    #[tokio::test]
    async fn test_http_store_file_reader_read() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv
            .mock("GET", "/test/file")
            .with_status(200)
            .with_header("Content-Type", "application/octet-stream")
            .with_body("Hello, world!")
            .create();
        let store = HttpStore::new(srv.url());
        let file = store.get_file("/test/file").await.unwrap();

        let reader = file.read(0..5).await.unwrap();

        let mut buf = vec![0; 5];
        let mut async_reader = tokio::io::BufReader::new(reader);
        let n = async_reader.read(&mut buf).await.unwrap();

        assert_eq!(n, 5);
        assert_eq!(&buf, b"Hello");
    }

    #[tokio::test]
    async fn test_http_store_file_reader_range_request() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv
            .mock("GET", "/test/file")
            .with_status(206) // Partial content status for range requests
            .with_header("Content-Type", "application/octet-stream")
            .with_header("Content-Range", "bytes 0-4/12")
            .with_body("Hello, world!")
            .create();

        let store = HttpStore::new(srv.url());
        let file = store.get_file("/test/file").await.unwrap();

        let reader = file.read(0..5).await.unwrap();
        let mut buf = vec![0; 5];

        let mut async_reader = tokio::io::BufReader::new(reader);
        let n = async_reader.read(&mut buf).await.unwrap();

        assert_eq!(n, 5);

        assert_eq!(&buf, b"Hello");
    }

    #[tokio::test]
    async fn test_http_store_file_reader_not_found() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv.mock("GET", "/test/file").with_status(404).create();

        let store = HttpStore::new(srv.url());
        let file = store.get_file("/test/file").await.unwrap();

        let result = file.read(0..5).await;
        match result {
            Ok(_) => panic!("should fail"),
            Err(err) => assert_eq!(err.kind(), ErrorKind::NotFound),
        }
    }
}
