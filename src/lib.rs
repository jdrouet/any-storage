#![doc = include_str!("../readme.md")]

use std::borrow::Cow;
use std::io::Result;
use std::ops::RangeBounds;
use std::path::PathBuf;

use futures::Stream;

/// Module wrapping all the implementations in an enum.
pub mod any;
/// Module for HTTP storage implementation.
pub mod http;
/// Module for local storage implementation.
pub mod local;
/// Module for pCloud storage implementation.
pub mod pcloud;

/// Utility module for common helper functions.
pub(crate) mod util;

/// Trait representing a generic storage system.
pub trait Store {
    /// Associated type for directories in the storage system.
    type Directory: StoreDirectory;
    /// Associated type for files in the storage system.
    type File: StoreFile;

    /// Returns the root directory of the store.
    ///
    /// The root is represented by a default (empty) `PathBuf`.
    fn root(&self) -> impl Future<Output = Result<Self::Directory>> {
        self.get_dir(PathBuf::default())
    }

    /// Retrieves a directory at the specified path.
    ///
    /// This method returns a future that resolves to the directory at the given
    /// path.
    fn get_dir<P: Into<PathBuf>>(&self, path: P) -> impl Future<Output = Result<Self::Directory>>;

    /// Retrieves a file at the specified path.
    ///
    /// This method returns a future that resolves to the file at the given
    /// path.
    fn get_file<P: Into<PathBuf>>(&self, path: P) -> impl Future<Output = Result<Self::File>>;
}

/// Trait representing a directory in the storage system.
pub trait StoreDirectory {
    /// Associated type for entries in the directory.
    type Entry;
    /// Associated type for the reader that iterates over the directory's
    /// entries.
    type Reader: StoreDirectoryReader<Self::Entry>;

    /// Checks if the directory exists.
    ///
    /// Returns a future that resolves to `true` if the directory exists,
    /// otherwise `false`.
    fn exists(&self) -> impl Future<Output = Result<bool>>;

    /// Reads the contents of the directory.
    ///
    /// Returns a future that resolves to a reader for the directory's entries.
    fn read(&self) -> impl Future<Output = Result<Self::Reader>>;
}

/// Trait for a reader that streams entries from a directory.
pub trait StoreDirectoryReader<E>: Stream<Item = Result<E>> + Sized {}

/// Trait representing a file in the storage system.
pub trait StoreFile {
    /// Associated type for the reader that reads the file's content.
    type FileReader: StoreFileReader;
    /// Associated type for the reader that reads the file's content.
    type FileWriter: StoreFileWriter;
    /// Associated type for the metadata associated with the file.
    type Metadata: StoreMetadata;

    /// Returns the file's name if it exists.
    ///
    /// This method returns an `Option` containing the file's name.
    fn filename(&self) -> Option<Cow<'_, str>>;

    /// Checks if the file exists.
    ///
    /// Returns a future that resolves to `true` if the file exists, otherwise
    /// `false`.
    fn exists(&self) -> impl Future<Output = Result<bool>>;

    /// Retrieves the metadata of the file.
    ///
    /// Returns a future that resolves to the file's metadata (size, creation
    /// time, etc.).
    fn metadata(&self) -> impl Future<Output = Result<Self::Metadata>>;

    /// Reads a portion of the file's content, specified by a byte range.
    ///
    /// Returns a future that resolves to a reader that can read the specified
    /// range of the file.
    fn read<R: RangeBounds<u64>>(&self, range: R)
    -> impl Future<Output = Result<Self::FileReader>>;

    /// Creates a writer
    fn write(&self, options: WriteOptions) -> impl Future<Output = Result<Self::FileWriter>>;
}

#[derive(Clone, Copy, Debug)]
enum WriteMode {
    Append,
    Truncate { offset: u64 },
}

#[derive(Clone, Debug)]
pub struct WriteOptions {
    mode: WriteMode,
}

impl WriteOptions {
    pub fn append() -> Self {
        Self {
            mode: WriteMode::Append,
        }
    }

    pub fn create() -> Self {
        Self {
            mode: WriteMode::Truncate { offset: 0 },
        }
    }

    pub fn truncate(offset: u64) -> Self {
        Self {
            mode: WriteMode::Truncate { offset },
        }
    }
}

/// Trait representing a reader that can asynchronously read the contents of a
/// file.
pub trait StoreFileReader: tokio::io::AsyncRead {}

/// Trait representing a writer that can asynchronously write the contents to a
/// file.
pub trait StoreFileWriter: tokio::io::AsyncWrite {}

/// Struct for stores that don't support writing
#[derive(Debug)]
pub struct NoopFileWriter;

impl StoreFileWriter for NoopFileWriter {}

impl tokio::io::AsyncWrite for NoopFileWriter {
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::task::Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "writer not supported for this store",
        )))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::task::Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "writer not supported for this store",
        )))
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        std::task::Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "writer not supported for this store",
        )))
    }
}

/// Enum representing either a file or a directory entry.
#[derive(Debug)]
pub enum Entry<File, Directory> {
    /// A file entry.
    File(File),
    /// A directory entry.
    Directory(Directory),
}

impl<File, Directory> Entry<File, Directory> {
    /// Returns `true` if the entry is a directory.
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }

    /// Returns `true` if the entry is a file.
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    /// Returns a reference to the directory if the entry is a directory.
    pub fn as_directory(&self) -> Option<&Directory> {
        match self {
            Self::Directory(inner) => Some(inner),
            _ => None,
        }
    }

    /// Returns a reference to the file if the entry is a file.
    pub fn as_file(&self) -> Option<&File> {
        match self {
            Self::File(inner) => Some(inner),
            _ => None,
        }
    }

    /// Converts the entry into a directory, returning an error if it’s not a
    /// directory.
    pub fn into_directory(self) -> std::result::Result<Directory, Self> {
        match self {
            Self::Directory(inner) => Ok(inner),
            other => Err(other),
        }
    }

    /// Converts the entry into a file, returning an error if it’s not a file.
    pub fn into_file(self) -> std::result::Result<File, Self> {
        match self {
            Self::File(inner) => Ok(inner),
            other => Err(other),
        }
    }
}

/// Trait representing the metadata of a file.
pub trait StoreMetadata {
    /// Returns the size of the file in bytes.
    fn size(&self) -> u64;

    /// Returns the creation timestamp of the file (epoch time).
    fn created(&self) -> u64;

    /// Returns the last modification timestamp of the file (epoch time).
    fn modified(&self) -> u64;
}

#[cfg(test)]
fn enable_tracing() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "INFO".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}
