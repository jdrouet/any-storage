use std::borrow::Cow;
use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use futures::Stream;
use pcloud::file::FileIdentifier;
use pcloud::folder::{FolderIdentifier, ROOT};
use reqwest::header;
use tokio::io::DuplexStream;
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;

use crate::WriteMode;
use crate::http::{HttpStoreFileReader, RangeHeader};

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum PCloudStoreConfigOrigin {
    Region { region: pcloud::Region },
    Url { url: Cow<'static, str> },
}

impl Default for PCloudStoreConfigOrigin {
    fn default() -> Self {
        Self::Region {
            region: pcloud::Region::Eu,
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct PCloudStoreConfig {
    #[cfg_attr(feature = "serde", serde(default, flatten))]
    pub origin: PCloudStoreConfigOrigin,
    pub credentials: pcloud::Credentials,
    #[cfg_attr(feature = "serde", serde(default))]
    pub root: PathBuf,
}

impl PCloudStoreConfig {
    pub fn build(&self) -> Result<PCloudStore> {
        let mut builder = pcloud::Client::builder();
        match self.origin {
            PCloudStoreConfigOrigin::Region { region } => {
                builder.set_region(region);
            }
            PCloudStoreConfigOrigin::Url { ref url } => {
                builder.set_base_url(url.clone());
            }
        };
        builder.set_credentials(self.credentials.clone());
        let client = builder
            .build()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        Ok(PCloudStore(Arc::new(InnerStore {
            client,
            root: self.root.clone(),
        })))
    }
}

struct InnerStore {
    client: pcloud::Client,
    root: PathBuf,
}

/// A store backed by the pCloud remote storage service.
#[derive(Clone)]
pub struct PCloudStore(Arc<InnerStore>);

impl std::fmt::Debug for PCloudStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PCloudStore))
            .finish_non_exhaustive()
    }
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

impl PCloudStore {
    /// Creates a new `PCloudStore` using a base URL and login credentials.
    pub fn new(
        base_url: impl Into<Cow<'static, str>>,
        credentials: pcloud::Credentials,
    ) -> Result<Self> {
        let client = pcloud::Client::builder()
            .with_base_url(base_url)
            .with_credentials(credentials)
            .build()
            .unwrap();
        Ok(Self(Arc::new(InnerStore {
            client,
            root: PathBuf::new(),
        })))
    }
}

impl crate::Store for PCloudStore {
    type Directory = PCloudStoreDirectory;
    type File = PCloudStoreFile;

    /// Retrieves a file handle for the given path in the pCloud store.
    async fn get_file<P: Into<PathBuf>>(&self, path: P) -> Result<Self::File> {
        Ok(PCloudStoreFile {
            store: self.0.clone(),
            path: path.into(),
        })
    }

    /// Retrieves a directory handle for the given path in the pCloud store.
    async fn get_dir<P: Into<PathBuf>>(&self, path: P) -> Result<Self::Directory> {
        Ok(PCloudStoreDirectory {
            store: self.0.clone(),
            path: path.into(),
        })
    }
}

// directory

/// A directory in the pCloud file store.
pub struct PCloudStoreDirectory {
    store: Arc<InnerStore>,
    path: PathBuf,
}

impl std::fmt::Debug for PCloudStoreDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PCloudStoreDirectory))
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl crate::StoreDirectory for PCloudStoreDirectory {
    type Entry = PCloudStoreEntry;
    type Reader = PCloudStoreDirectoryReader;

    /// Checks if the directory exists on pCloud.
    async fn exists(&self) -> Result<bool> {
        let identifier = FolderIdentifier::path(self.path.to_string_lossy());
        match self.store.client.list_folder(identifier).await {
            Ok(_) => Ok(true),
            Err(pcloud::Error::Protocol(2005, _)) => Ok(false),
            Err(other) => Err(Error::other(other)),
        }
    }

    /// Reads the directory contents from pCloud and returns an entry reader.
    async fn read(&self) -> Result<Self::Reader> {
        let path = crate::util::merge_path(&self.store.root, &self.path)?;
        let identifier = FolderIdentifier::path(path.to_string_lossy());
        match self.store.client.list_folder(identifier).await {
            Ok(folder) => Ok(PCloudStoreDirectoryReader {
                store: self.store.clone(),
                path: self.path.clone(),
                entries: folder.contents.unwrap_or_default(),
            }),
            Err(pcloud::Error::Protocol(2005, _)) => {
                Err(Error::new(ErrorKind::NotFound, "directory not found"))
            }
            Err(other) => Err(Error::other(other)),
        }
    }
}

/// A streaming reader over entries in a pCloud directory.
pub struct PCloudStoreDirectoryReader {
    store: Arc<InnerStore>,
    path: PathBuf,
    entries: Vec<pcloud::entry::Entry>,
}

impl std::fmt::Debug for PCloudStoreDirectoryReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PCloudStoreDirectoryReader))
            .field("path", &self.path)
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

impl Stream for PCloudStoreDirectoryReader {
    type Item = Result<PCloudStoreEntry>;

    /// Polls the next entry in the directory listing.
    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut();

        if let Some(entry) = this.entries.pop() {
            Poll::Ready(Some(PCloudStoreEntry::new(
                self.store.clone(),
                self.path.clone(),
                entry,
            )))
        } else {
            Poll::Ready(None)
        }
    }
}

impl crate::StoreDirectoryReader<PCloudStoreEntry> for PCloudStoreDirectoryReader {}

// files

/// A file in the pCloud file store.
pub struct PCloudStoreFile {
    store: Arc<InnerStore>,
    path: PathBuf,
}

impl std::fmt::Debug for PCloudStoreFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PCloudStoreFile))
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl crate::StoreFile for PCloudStoreFile {
    type FileReader = PCloudStoreFileReader;
    type FileWriter = PCloudStoreFileWriter;
    type Metadata = PCloudStoreFileMetadata;

    /// Returns the filename portion of the file's path.
    fn filename(&self) -> Option<Cow<'_, str>> {
        let cmp = self.path.components().next_back()?;
        Some(cmp.as_os_str().to_string_lossy())
    }

    /// Checks whether the file exists on pCloud.
    async fn exists(&self) -> Result<bool> {
        let path = crate::util::merge_path(&self.store.root, &self.path)?;
        let identifier = FileIdentifier::path(path.to_string_lossy());
        match self.store.client.get_file_checksum(identifier).await {
            Ok(_) => Ok(true),
            Err(pcloud::Error::Protocol(2009, _)) => Ok(false),
            Err(other) => Err(Error::other(other)),
        }
    }

    /// Retrieves metadata about the file (size, creation, and modification
    /// times).
    async fn metadata(&self) -> Result<Self::Metadata> {
        let path = crate::util::merge_path(&self.store.root, &self.path)?;
        let identifier = FileIdentifier::path(path.to_string_lossy());
        match self.store.client.get_file_checksum(identifier).await {
            Ok(file) => Ok(PCloudStoreFileMetadata {
                size: file.metadata.size.unwrap_or(0) as u64,
                created: file.metadata.base.created.timestamp() as u64,
                modified: file.metadata.base.modified.timestamp() as u64,
            }),
            Err(pcloud::Error::Protocol(2009, _)) => {
                Err(Error::new(ErrorKind::NotFound, "file not found"))
            }
            Err(other) => Err(Error::other(other)),
        }
    }

    /// Reads a byte range of the file content using a download link from
    /// pCloud.
    async fn read<R: std::ops::RangeBounds<u64>>(&self, range: R) -> Result<Self::FileReader> {
        let path = crate::util::merge_path(&self.store.root, &self.path)?;
        let identifier = FileIdentifier::path(path.to_string_lossy());
        let links = self
            .store
            .client
            .get_file_link(identifier)
            .await
            .map_err(|err| match err {
                pcloud::Error::Protocol(2009, _) => {
                    Error::new(ErrorKind::NotFound, "file not found")
                }
                other => Error::other(other),
            })?;
        let link = links
            .first_link()
            .ok_or_else(|| Error::other("unable to fetch file link"))?;
        let url = link.to_string();
        let res = reqwest::Client::new()
            .get(url)
            .header(header::RANGE, RangeHeader(range).to_string())
            .header(header::USER_AGENT, APP_USER_AGENT)
            .send()
            .await
            .map_err(Error::other)?;
        PCloudStoreFileReader::from_response(res)
    }

    /// Creates a writer to a file in pcloud
    async fn write(&self, options: crate::WriteOptions) -> Result<Self::FileWriter> {
        match options.mode {
            WriteMode::Append => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "pcloud store doesn't support append write",
                ));
            }
            WriteMode::Truncate { offset } if offset != 0 => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "pcloud store doesn't support truncated write",
                ));
            }
            _ => {}
        };

        let path = crate::util::merge_path(&self.store.root, &self.path)?;
        let parent: FolderIdentifier<'static> = path
            .parent()
            .map(|parent| parent.to_path_buf())
            .map(|parent| FolderIdentifier::path(parent.to_string_lossy().to_string()))
            .unwrap_or_else(|| FolderIdentifier::FolderId(ROOT));
        let filename = path
            .file_name()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "unable to get file name"))?;
        let filename = filename.to_string_lossy().to_string();

        // TODO find a way to make the 8KB buffer a parameter
        let (write_buffer, read_buffer) = tokio::io::duplex(8192);

        let client = self.store.clone();
        let stream = ReaderStream::new(read_buffer);
        let files = pcloud::file::upload::MultiFileUpload::default()
            .with_stream_entry(filename, None, stream);

        // spawn a task that will keep the request connected while we are pushing data
        let upload_task: JoinHandle<Result<()>> = tokio::spawn(async move {
            client
                .client
                .upload_files(parent, files)
                .await
                .map(|_| ())
                .map_err(Error::other)
        });

        Ok(PCloudStoreFileWriter {
            write_buffer,
            upload_task,
        })
    }
}

/// Writer to PCloud file
#[derive(Debug)]
pub struct PCloudStoreFileWriter {
    write_buffer: DuplexStream,
    upload_task: JoinHandle<Result<()>>,
}

impl tokio::io::AsyncWrite for PCloudStoreFileWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        if self.upload_task.is_finished() {
            Poll::Ready(Err(Error::new(ErrorKind::BrokenPipe, "request closed")))
        } else {
            Pin::new(&mut self.write_buffer).poll_write(cx, buf)
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        if self.upload_task.is_finished() {
            Poll::Ready(Err(Error::new(ErrorKind::BrokenPipe, "request closed")))
        } else {
            Pin::new(&mut self.write_buffer).poll_flush(cx)
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<()>> {
        let shutdown = Pin::new(&mut self.write_buffer).poll_shutdown(cx);

        if shutdown.is_ready() {
            let poll = Pin::new(&mut self.upload_task).poll(cx);
            match poll {
                Poll::Ready(Ok(res)) => Poll::Ready(res),
                Poll::Ready(Err(err)) => Poll::Ready(Err(Error::other(err))),
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    }
}

impl crate::StoreFileWriter for PCloudStoreFileWriter {}

/// Metadata for a file in the pCloud store.
#[derive(Clone, Debug)]
pub struct PCloudStoreFileMetadata {
    size: u64,
    created: u64,
    modified: u64,
}

impl super::StoreMetadata for PCloudStoreFileMetadata {
    /// Returns the file size in bytes.
    fn size(&self) -> u64 {
        self.size
    }

    /// Returns the UNIX timestamp when the file was created.
    fn created(&self) -> u64 {
        self.created
    }

    /// Returns the UNIX timestamp when the file was last modified.
    fn modified(&self) -> u64 {
        self.modified
    }
}

/// File reader type for pCloud files.
///
/// Reuses `HttpStoreFileReader` for actual byte streaming via HTTP.
pub type PCloudStoreFileReader = HttpStoreFileReader;

/// Represents a file or directory entry within the pCloud store.
pub type PCloudStoreEntry = crate::Entry<PCloudStoreFile, PCloudStoreDirectory>;

impl PCloudStoreEntry {
    /// Constructs a `PCloudStoreEntry` from a parent path and a pCloud entry.
    ///
    /// Determines if the entry is a file or directory.
    fn new(store: Arc<InnerStore>, parent: PathBuf, entry: pcloud::entry::Entry) -> Result<Self> {
        let path = parent.join(&entry.base().name);
        Ok(match entry {
            pcloud::entry::Entry::File(_) => Self::File(PCloudStoreFile { store, path }),
            pcloud::entry::Entry::Folder(_) => {
                Self::Directory(PCloudStoreDirectory { store, path })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use mockito::Matcher;
    use tokio::io::AsyncWriteExt;

    use super::*;
    use crate::{Store, StoreFile, WriteOptions};

    #[tokio::test]
    async fn should_write_file() {
        crate::enable_tracing();
        let content = include_bytes!("lib.rs");
        let mut srv = mockito::Server::new_async().await;
        let mock = srv
            .mock("POST", "/uploadfile")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("username".into(), "username".into()),
                Matcher::UrlEncoded("password".into(), "password".into()),
                Matcher::UrlEncoded("path".into(), "/foo".into()),
            ]))
            .match_header(
                "content-type",
                Matcher::Regex("multipart/form-data; boundary=.*".to_string()),
            )
            .match_body(Matcher::Any)
            .with_status(200)
            // we don't care about the body
            .with_body(r#"{"result": 0, "metadata": [], "checksums": [], "fileids": []}"#)
            .create_async()
            .await;

        let store = PCloudStore::new(
            srv.url(),
            Credentials {
                username: "username".into(),
                password: "password".into(),
            },
        )
        .unwrap();
        let file = store.get_file("/foo/bar.txt").await.unwrap();
        let mut writer = file.write(WriteOptions::create()).await.unwrap();
        writer.write_all(content).await.unwrap();
        writer.shutdown().await.unwrap();
        mock.assert_async().await;
    }
}
