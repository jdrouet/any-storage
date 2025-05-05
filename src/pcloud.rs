use std::borrow::Cow;
use std::io::{Error, ErrorKind, Result};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use futures::Stream;
use pcloud::file::FileIdentifier;
use pcloud::folder::FolderIdentifier;
use reqwest::header;

use crate::http::{HttpStoreFileReader, RangeHeader};

/// Stores username and password credentials for authentication.
pub struct Credentials {
    username: Box<str>,
    password: Box<str>,
}

impl std::fmt::Debug for Credentials {
    /// Omits the password field from debug output for security.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Credentials))
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// A store backed by the pCloud remote storage service.
pub struct PCloudStore(Arc<pcloud::Client>);

impl std::fmt::Debug for PCloudStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PCloudStore))
            .finish_non_exhaustive()
    }
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

impl PCloudStore {
    /// Creates a new `PCloudStore` using a base URL and login credentials.
    pub fn new(base_url: impl Into<Cow<'static, str>>, credentials: Credentials) -> Result<Self> {
        let client = pcloud::Client::builder()
            .with_base_url(base_url)
            .with_credentials(pcloud::Credentials::UsernamePassword {
                username: credentials.username.to_string(),
                password: credentials.password.to_string(),
            })
            .build()
            .unwrap();
        Ok(Self(Arc::new(client)))
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
    store: Arc<pcloud::Client>,
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
        match self.store.list_folder(identifier).await {
            Ok(_) => Ok(true),
            Err(pcloud::Error::Protocol(2005, _)) => Ok(false),
            Err(other) => Err(Error::other(other)),
        }
    }

    /// Reads the directory contents from pCloud and returns an entry reader.
    async fn read(&self) -> Result<Self::Reader> {
        let identifier = FolderIdentifier::path(self.path.to_string_lossy());
        match self.store.list_folder(identifier).await {
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
    store: Arc<pcloud::Client>,
    path: PathBuf,
    entries: Vec<pcloud::entry::Entry>,
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
    store: Arc<pcloud::Client>,
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
    type Metadata = PCloudStoreFileMetadata;

    /// Returns the filename portion of the file's path.
    fn filename(&self) -> Option<Cow<'_, str>> {
        let cmp = self.path.components().last()?;
        Some(cmp.as_os_str().to_string_lossy())
    }

    /// Checks whether the file exists on pCloud.
    async fn exists(&self) -> Result<bool> {
        let identifier = FileIdentifier::path(self.path.to_string_lossy());
        match self.store.get_file_checksum(identifier).await {
            Ok(_) => Ok(true),
            Err(pcloud::Error::Protocol(2009, _)) => Ok(false),
            Err(other) => Err(Error::other(other)),
        }
    }

    /// Retrieves metadata about the file (size, creation, and modification
    /// times).
    async fn metadata(&self) -> Result<Self::Metadata> {
        let identifier = FileIdentifier::path(self.path.to_string_lossy());
        match self.store.get_file_checksum(identifier).await {
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
        let identifier = FileIdentifier::path(self.path.to_string_lossy());
        let links = self
            .store
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
}

/// Metadata for a file in the pCloud store.
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
    fn new(
        store: Arc<pcloud::Client>,
        parent: PathBuf,
        entry: pcloud::entry::Entry,
    ) -> Result<Self> {
        let path = parent.join(&entry.base().name);
        Ok(match entry {
            pcloud::entry::Entry::File(_) => Self::File(PCloudStoreFile { store, path }),
            pcloud::entry::Entry::Folder(_) => {
                Self::Directory(PCloudStoreDirectory { store, path })
            }
        })
    }
}
