use std::borrow::Cow;
use std::io::{Error, ErrorKind, Result};
use std::ops::{Bound, RangeBounds};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::header::{CONTENT_LENGTH, LAST_MODIFIED};
use reqwest::{StatusCode, Url, header};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;

mod parser;

/// Converts an HTTP status code into a `Result`, returning an `io::Error`
/// for client or server errors, and `Ok(code)` otherwise.
pub(crate) fn error_from_status(code: StatusCode) -> Result<StatusCode> {
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

/// Helper struct to format HTTP Range headers from a `RangeBounds<u64>`.
pub(crate) struct RangeHeader<R: RangeBounds<u64>>(pub R);

impl<R: RangeBounds<u64>> std::fmt::Display for RangeHeader<R> {
    /// Formats the HTTP `Range` header value (e.g., "bytes=0-100").
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

/// Internal representation of the HTTP-backed store.
struct InnerHttpStore {
    base_url: Url,
    parser: parser::Parser,
    client: reqwest::Client,
}

impl InnerHttpStore {
    /// Resolves a relative file or directory path into a full URL.
    fn get_url(&self, path: &Path) -> Result<Url> {
        let clean = crate::util::clean_path(path)?;
        self.base_url
            .join(&clean.to_string_lossy())
            .map_err(|err| Error::new(ErrorKind::InvalidData, err))
    }
}

/// Public HTTP-backed file store supporting asynchronous access to remote files
/// and directories.
#[derive(Clone)]
pub struct HttpStore(Arc<InnerHttpStore>);

impl std::fmt::Debug for HttpStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HttpStore))
            .field("base_url", &self.0.base_url)
            .finish_non_exhaustive()
    }
}

impl HttpStore {
    /// Creates a new `HttpStore` from a base URL.
    ///
    /// Ensures the base URL ends with a trailing slash and initializes the HTTP
    /// client and parser.
    pub fn new(base_url: impl AsRef<str>) -> Result<Self> {
        let base_url = base_url.as_ref();
        let base_url = if base_url.ends_with("/") {
            Cow::Borrowed(base_url)
        } else {
            Cow::Owned(format!("{base_url}/"))
        };
        let base_url = Url::parse(base_url.as_ref())
            .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
        Ok(Self(Arc::new(InnerHttpStore {
            base_url: base_url.into(),
            parser: parser::Parser::default(),
            client: reqwest::Client::new(),
        })))
    }
}

impl crate::Store for HttpStore {
    type Directory = HttpStoreDirectory;
    type File = HttpStoreFile;

    /// Retrieves a file from the HTTP store at the given path.
    async fn get_file<P: Into<std::path::PathBuf>>(&self, path: P) -> Result<Self::File> {
        Ok(HttpStoreFile {
            store: self.0.clone(),
            path: path.into(),
        })
    }

    /// Retrieves a directory from the HTTP store at the given path.
    async fn get_dir<P: Into<PathBuf>>(&self, path: P) -> Result<Self::Directory> {
        Ok(HttpStoreDirectory {
            store: self.0.clone(),
            path: path.into(),
        })
    }
}

/// Representation of a directory in the HTTP store.
pub struct HttpStoreDirectory {
    store: Arc<InnerHttpStore>,
    path: PathBuf,
}

impl std::fmt::Debug for HttpStoreDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HttpStoreDirectory))
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl crate::StoreDirectory for HttpStoreDirectory {
    type Entry = HttpStoreEntry;
    type Reader = HttpStoreDirectoryReader;

    /// Checks if the HTTP directory exists via a HEAD request.
    async fn exists(&self) -> Result<bool> {
        let url = self.store.get_url(&self.path)?;
        match self.store.client.head(url).send().await {
            Ok(res) => match res.status() {
                StatusCode::NOT_FOUND => Ok(false),
                other => error_from_status(other).map(|_| true),
            },
            Err(err) => Err(Error::other(err)),
        }
    }

    /// Lists the entries in the HTTP directory by fetching and parsing HTML.
    async fn read(&self) -> Result<Self::Reader> {
        let url = self.store.get_url(&self.path)?;
        let res = self
            .store
            .client
            .get(url)
            .send()
            .await
            .map_err(Error::other)?;
        error_from_status(res.status())?;
        let html = res.text().await.map_err(Error::other)?;
        let mut entries = self.store.parser.parse(&html).collect::<Vec<_>>();
        entries.reverse();

        Ok(HttpStoreDirectoryReader {
            store: self.store.clone(),
            path: self.path.clone(),
            entries,
        })
    }
}

/// Stream reader over entries within an HTTP directory listing.
pub struct HttpStoreDirectoryReader {
    store: Arc<InnerHttpStore>,
    path: PathBuf,
    entries: Vec<String>,
}

impl Stream for HttpStoreDirectoryReader {
    type Item = Result<HttpStoreEntry>;

    /// Returns the next directory entry from the parsed HTML listing.
    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut();

        if let Some(entry) = this.entries.pop() {
            Poll::Ready(Some(HttpStoreEntry::new(
                self.store.clone(),
                self.path.clone(),
                entry,
            )))
        } else {
            Poll::Ready(None)
        }
    }
}

impl crate::StoreDirectoryReader<HttpStoreEntry> for HttpStoreDirectoryReader {}

/// Representation of a file in the HTTP store.
pub struct HttpStoreFile {
    store: Arc<InnerHttpStore>,
    path: PathBuf,
}

impl std::fmt::Debug for HttpStoreFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(HttpStoreFile))
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl crate::StoreFile for HttpStoreFile {
    type FileReader = HttpStoreFileReader;
    type Metadata = HttpStoreFileMetadata;

    /// Returns the filename portion of the HTTP path.
    fn filename(&self) -> Option<Cow<'_, str>> {
        let cmp = self.path.components().last()?;
        Some(cmp.as_os_str().to_string_lossy())
    }

    /// Checks if the HTTP file exists via a HEAD request.
    async fn exists(&self) -> Result<bool> {
        let url = self.store.get_url(&self.path)?;
        let res = self
            .store
            .client
            .head(url)
            .send()
            .await
            .map_err(Error::other)?;
        match res.status() {
            StatusCode::NOT_FOUND => Ok(false),
            other => error_from_status(other).map(|_| true),
        }
    }

    /// Retrieves the HTTP file metadata (size and last modified).
    async fn metadata(&self) -> Result<Self::Metadata> {
        let url = self.store.get_url(&self.path)?;
        let res = self
            .store
            .client
            .head(url)
            .send()
            .await
            .map_err(Error::other)?;
        error_from_status(res.status())?;
        let size = res
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let modified = res
            .headers()
            .get(LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| OffsetDateTime::parse(value, &Rfc2822).ok())
            .map(|dt| dt.unix_timestamp() as u64)
            .unwrap_or(0);
        Ok(HttpStoreFileMetadata { size, modified })
    }

    /// Begins reading a file from the HTTP store for the given byte range.
    async fn read<R: std::ops::RangeBounds<u64>>(&self, range: R) -> Result<Self::FileReader> {
        let url = self.store.get_url(&self.path)?;
        let res = self
            .store
            .client
            .get(url)
            .header(header::RANGE, RangeHeader(range).to_string())
            .send()
            .await
            .map_err(Error::other)?;
        HttpStoreFileReader::from_response(res)
    }
}

/// Metadata for an HTTP file, containing size and last modification time.
pub struct HttpStoreFileMetadata {
    size: u64,
    modified: u64,
}

impl super::StoreMetadata for HttpStoreFileMetadata {
    /// Returns the file size in bytes.
    fn size(&self) -> u64 {
        self.size
    }

    /// Returns 0 as creation time is not available over HTTP.
    fn created(&self) -> u64 {
        0
    }

    /// Returns the last modified time (as a UNIX timestamp).
    fn modified(&self) -> u64 {
        self.modified
    }
}

/// Reader for streaming bytes from a remote HTTP file.
pub struct HttpStoreFileReader {
    stream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + std::marker::Send>>,
}

impl HttpStoreFileReader {
    /// Creates a `HttpStoreFileReader` from a `reqwest::Response`.
    ///
    /// Validates the response and initializes the byte stream.
    pub(crate) fn from_response(res: reqwest::Response) -> Result<Self> {
        crate::http::error_from_status(res.status())?;
        // TODO handle when status code is not 206
        let stream = res.bytes_stream().boxed();
        Ok(Self { stream })
    }
}

impl tokio::io::AsyncRead for HttpStoreFileReader {
    /// Polls the next chunk of data from the HTTP byte stream.
    ///
    /// Copies bytes into the provided buffer.
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

/// Represents an entry in the HTTP store (file or directory).
pub type HttpStoreEntry = crate::Entry<HttpStoreFile, HttpStoreDirectory>;

impl HttpStoreEntry {
    /// Constructs a new `HttpStoreEntry` (either file or directory) from a path
    /// component.
    ///
    /// Assumes directory entries end with a `/`.
    fn new(store: Arc<InnerHttpStore>, parent: PathBuf, entry: String) -> Result<Self> {
        let path = parent.join(&entry);
        Ok(if entry.ends_with('/') {
            Self::Directory(HttpStoreDirectory { store, path })
        } else {
            Self::File(HttpStoreFile { store, path })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::PathBuf;

    use futures::StreamExt;
    use reqwest::header::{CONTENT_LENGTH, LAST_MODIFIED};
    use tokio::io::AsyncReadExt;

    use crate::http::HttpStore;
    use crate::{Store, StoreDirectory, StoreFile, StoreMetadata};

    #[test_case::test_case("http://localhost", "/foo.txt", "http://localhost/foo.txt"; "root with simple path with prefix")]
    #[test_case::test_case("http://localhost", "foo.txt", "http://localhost/foo.txt"; "root with simple path without prefix")]
    #[test_case::test_case("http://localhost/", "foo.txt", "http://localhost/foo.txt"; "root with simple path with slash on base")]
    #[test_case::test_case("http://localhost/", "/foo.txt", "http://localhost/foo.txt"; "root with simple path with slashes")]
    #[test_case::test_case("http://localhost/foo", "/bar/baz.txt", "http://localhost/foo/bar/baz.txt"; "with more children")]
    #[test_case::test_case("http://localhost/foo", "/bar/with space.txt", "http://localhost/foo/bar/with%20space.txt"; "with spaces")]
    fn building_path(base_url: &str, path: &str, expected: &str) {
        let store = HttpStore::new(base_url).unwrap();
        let path = PathBuf::from(path);
        let url = store.0.get_url(&path).unwrap();
        assert_eq!(url.as_str(), expected);
    }

    #[tokio::test]
    async fn file_should_handle_base_with_ending_slash() {
        let mut srv = mockito::Server::new_async().await;
        let mock = srv
            .mock("HEAD", "/foo/not-found.txt")
            .with_status(404)
            .create_async()
            .await;
        let store = HttpStore::new(format!("{}/foo/", srv.url())).unwrap();
        let file = store.get_file("/not-found.txt").await.unwrap();
        assert!(!file.exists().await.unwrap());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn file_should_check_if_file_exists() {
        let mut srv = mockito::Server::new_async().await;
        let mock = srv
            .mock("HEAD", "/not-found.txt")
            .with_status(404)
            .create_async()
            .await;
        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/not-found.txt").await.unwrap();
        assert!(!file.exists().await.unwrap());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn file_should_get_filename() {
        let srv = mockito::Server::new_async().await;
        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/file.txt").await.unwrap();
        let name = file.filename().unwrap();
        assert_eq!(name, "file.txt");
    }

    #[tokio::test]
    async fn file_should_get_filename_with_space() {
        let srv = mockito::Server::new_async().await;
        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/with space.txt").await.unwrap();
        let name = file.filename().unwrap();
        assert_eq!(name, "with space.txt");
    }

    #[tokio::test]
    async fn file_meta_should_give_all() {
        let mut srv = mockito::Server::new_async().await;
        let mock = srv
            .mock("HEAD", "/test/file.txt")
            .with_status(200)
            .with_header(CONTENT_LENGTH, "1234")
            .with_header(LAST_MODIFIED, "Thu, 01 May 2025 09:57:28 GMT")
            .create_async()
            .await;
        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/file.txt").await.unwrap();
        let meta = file.metadata().await.unwrap();
        assert_eq!(meta.size, 1234);
        assert_eq!(meta.created(), 0);
        assert_eq!(meta.modified(), 1746093448);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn file_reader_should_read_entire_file() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv
            .mock("GET", "/test/file")
            .with_status(200)
            .with_header("Content-Type", "application/octet-stream")
            .with_body("Hello, world!")
            .create();
        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/file").await.unwrap();

        let reader = file.read(0..5).await.unwrap();

        let mut buf = vec![0; 5];
        let mut async_reader = tokio::io::BufReader::new(reader);
        let n = async_reader.read(&mut buf).await.unwrap();

        assert_eq!(n, 5);
        assert_eq!(&buf, b"Hello");
    }

    #[tokio::test]
    async fn file_reader_should_read_single_range() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv
            .mock("GET", "/test/file")
            .with_status(206) // Partial content status for range requests
            .with_header("Content-Type", "application/octet-stream")
            .with_header("Content-Range", "bytes 0-4/12")
            .with_body("Hello, world!")
            .create();

        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/file").await.unwrap();

        let reader = file.read(0..5).await.unwrap();
        let mut buf = vec![0; 5];

        let mut async_reader = tokio::io::BufReader::new(reader);
        let n = async_reader.read(&mut buf).await.unwrap();

        assert_eq!(n, 5);

        assert_eq!(&buf, b"Hello");
    }

    #[tokio::test]
    async fn file_reader_should_fail_with_not_found() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv.mock("GET", "/test/file").with_status(404).create();

        let store = HttpStore::new(srv.url()).unwrap();
        let file = store.get_file("/test/file").await.unwrap();

        let result = file.read(0..5).await;
        match result {
            Ok(_) => panic!("should fail"),
            Err(err) => assert_eq!(err.kind(), ErrorKind::NotFound),
        }
    }

    #[tokio::test]
    async fn dir_should_list_entries() {
        let mut srv = mockito::Server::new_async().await;
        let _m = srv
            .mock("GET", "/NEH")
            .with_status(200)
            .with_body(include_str!("../../assets/apache.html"))
            .create();

        let store = HttpStore::new(srv.url()).unwrap();
        let dir = store.get_dir("/NEH").await.unwrap();
        let mut content = dir.read().await.unwrap();

        let mut result = Vec::new();
        while let Some(entry) = content.next().await {
            result.push(entry.unwrap());
        }
        assert_eq!(result.len(), 46);

        assert_eq!(result.iter().filter(|item| item.is_directory()).count(), 41);
        assert_eq!(result.iter().filter(|item| item.is_file()).count(), 5);
    }
}
