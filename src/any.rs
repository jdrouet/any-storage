use std::borrow::Cow;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::StreamExt;

#[derive(Clone, Debug, derive_more::From)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "lowercase"))]
#[non_exhaustive]
pub enum AnyStoreConfig {
    Http(crate::http::HttpStoreConfig),
    Local(crate::local::LocalStoreConfig),
    Noop(crate::noop::NoopStoreConfig),
    PCloud(crate::pcloud::PCloudStoreConfig),
}

impl AnyStoreConfig {
    /// Create a [`AnyStore`] based on the configuration
    pub fn build(&self) -> std::io::Result<AnyStore> {
        match self {
            Self::Http(inner) => inner.build().map(AnyStore::Http),
            Self::Local(inner) => inner.build().map(AnyStore::Local),
            Self::Noop(inner) => inner.build().map(AnyStore::Noop),
            Self::PCloud(inner) => inner.build().map(AnyStore::PCloud),
        }
    }
}

#[derive(Clone, Debug, derive_more::From)]
#[non_exhaustive]
pub enum AnyStore {
    Http(crate::http::HttpStore),
    Local(crate::local::LocalStore),
    Noop(crate::noop::NoopStore),
    PCloud(crate::pcloud::PCloudStore),
}

impl crate::Store for AnyStore {
    type File = AnyStoreFile;
    type Directory = AnyStoreDirectory;

    async fn root(&self) -> Result<Self::Directory> {
        match self {
            Self::Http(inner) => inner.root().await.map(AnyStoreDirectory::Http),
            Self::Local(inner) => inner.root().await.map(AnyStoreDirectory::Local),
            Self::Noop(inner) => inner.root().await.map(AnyStoreDirectory::Noop),
            Self::PCloud(inner) => inner.root().await.map(AnyStoreDirectory::PCloud),
        }
    }

    async fn get_dir<P: Into<std::path::PathBuf>>(&self, path: P) -> Result<Self::Directory> {
        match self {
            Self::Http(inner) => inner.get_dir(path).await.map(AnyStoreDirectory::Http),
            Self::Local(inner) => inner.get_dir(path).await.map(AnyStoreDirectory::Local),
            Self::Noop(inner) => inner.get_dir(path).await.map(AnyStoreDirectory::Noop),
            Self::PCloud(inner) => inner.get_dir(path).await.map(AnyStoreDirectory::PCloud),
        }
    }

    async fn get_file<P: Into<std::path::PathBuf>>(&self, path: P) -> Result<Self::File> {
        match self {
            Self::Http(inner) => inner.get_file(path).await.map(AnyStoreFile::Http),
            Self::Local(inner) => inner.get_file(path).await.map(AnyStoreFile::Local),
            Self::Noop(inner) => inner.get_file(path).await.map(AnyStoreFile::Noop),
            Self::PCloud(inner) => inner.get_file(path).await.map(AnyStoreFile::PCloud),
        }
    }
}

#[derive(Debug, derive_more::From)]
#[non_exhaustive]
pub enum AnyStoreFile {
    Http(crate::http::HttpStoreFile),
    Local(crate::local::LocalStoreFile),
    Noop(crate::noop::NoopStoreFile),
    PCloud(crate::pcloud::PCloudStoreFile),
}

impl crate::StoreFile for AnyStoreFile {
    type FileReader = AnyStoreFileReader;
    type FileWriter = AnyStoreFileWriter;
    type Metadata = AnyStoreFileMetadata;

    fn path(&self) -> &std::path::Path {
        match self {
            Self::Http(inner) => inner.path(),
            Self::Local(inner) => inner.path(),
            Self::Noop(inner) => inner.path(),
            Self::PCloud(inner) => inner.path(),
        }
    }

    async fn exists(&self) -> Result<bool> {
        match self {
            Self::Http(inner) => inner.exists().await,
            Self::Local(inner) => inner.exists().await,
            Self::Noop(inner) => inner.exists().await,
            Self::PCloud(inner) => inner.exists().await,
        }
    }

    fn filename(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::Http(inner) => inner.filename(),
            Self::Local(inner) => inner.filename(),
            Self::Noop(inner) => inner.filename(),
            Self::PCloud(inner) => inner.filename(),
        }
    }

    async fn metadata(&self) -> Result<Self::Metadata> {
        match self {
            Self::Http(inner) => inner.metadata().await.map(AnyStoreFileMetadata::Http),
            Self::Local(inner) => inner.metadata().await.map(AnyStoreFileMetadata::Local),
            Self::Noop(inner) => inner.metadata().await.map(AnyStoreFileMetadata::Noop),
            Self::PCloud(inner) => inner.metadata().await.map(AnyStoreFileMetadata::PCloud),
        }
    }

    async fn read<R: std::ops::RangeBounds<u64>>(&self, range: R) -> Result<Self::FileReader> {
        match self {
            Self::Http(inner) => inner.read(range).await.map(AnyStoreFileReader::Http),
            Self::Local(inner) => inner.read(range).await.map(AnyStoreFileReader::Local),
            Self::Noop(inner) => inner.read(range).await.map(AnyStoreFileReader::Noop),
            Self::PCloud(inner) => inner.read(range).await.map(AnyStoreFileReader::PCloud),
        }
    }

    async fn write(&self, options: crate::WriteOptions) -> Result<Self::FileWriter> {
        match self {
            Self::Http(inner) => inner.write(options).await.map(AnyStoreFileWriter::Http),
            Self::Local(inner) => inner.write(options).await.map(AnyStoreFileWriter::Local),
            Self::Noop(inner) => inner.write(options).await.map(AnyStoreFileWriter::Noop),
            Self::PCloud(inner) => inner.write(options).await.map(AnyStoreFileWriter::PCloud),
        }
    }

    async fn delete(&self) -> Result<()> {
        match self {
            Self::Http(inner) => inner.delete().await,
            Self::Local(inner) => inner.delete().await,
            Self::Noop(inner) => inner.delete().await,
            Self::PCloud(inner) => inner.delete().await,
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AnyStoreFileReader {
    Http(crate::http::HttpStoreFileReader),
    Local(crate::local::LocalStoreFileReader),
    Noop(crate::noop::NoopStoreFileReader),
    PCloud(crate::pcloud::PCloudStoreFileReader),
}

impl tokio::io::AsyncRead for AnyStoreFileReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let this = self.get_mut();
        match this {
            Self::Http(inner) => Pin::new(inner).poll_read(cx, buf),
            Self::Local(inner) => Pin::new(inner).poll_read(cx, buf),
            Self::Noop(inner) => Pin::new(inner).poll_read(cx, buf),
            Self::PCloud(inner) => Pin::new(inner).poll_read(cx, buf),
        }
    }
}

impl crate::StoreFileReader for AnyStoreFileReader {}

#[derive(Clone, Debug, derive_more::From)]
#[non_exhaustive]
pub enum AnyStoreFileMetadata {
    Http(crate::http::HttpStoreFileMetadata),
    Local(crate::local::LocalStoreFileMetadata),
    Noop(crate::noop::NoopStoreFileMetadata),
    PCloud(crate::pcloud::PCloudStoreFileMetadata),
}

impl crate::StoreMetadata for AnyStoreFileMetadata {
    fn created(&self) -> u64 {
        match self {
            Self::Http(inner) => inner.created(),
            Self::Local(inner) => inner.created(),
            Self::Noop(inner) => inner.created(),
            Self::PCloud(inner) => inner.created(),
        }
    }

    fn modified(&self) -> u64 {
        match self {
            Self::Http(inner) => inner.modified(),
            Self::Local(inner) => inner.modified(),
            Self::Noop(inner) => inner.modified(),
            Self::PCloud(inner) => inner.modified(),
        }
    }

    fn size(&self) -> u64 {
        match self {
            Self::Http(inner) => inner.size(),
            Self::Local(inner) => inner.size(),
            Self::Noop(inner) => inner.size(),
            Self::PCloud(inner) => inner.size(),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AnyStoreFileWriter {
    Http(crate::noop::NoopStoreFileWriter),
    Noop(crate::noop::NoopStoreFileWriter),
    Local(crate::local::LocalStoreFileWriter),
    PCloud(crate::pcloud::PCloudStoreFileWriter),
}

impl tokio::io::AsyncWrite for AnyStoreFileWriter {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        let this = self.get_mut();

        match this {
            Self::Http(inner) | Self::Noop(inner) => Pin::new(inner).poll_write(cx, buf),
            Self::Local(inner) => Pin::new(inner).poll_write(cx, buf),
            Self::PCloud(inner) => Pin::new(inner).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        let this = self.get_mut();

        match this {
            Self::Http(inner) | Self::Noop(inner) => Pin::new(inner).poll_flush(cx),
            Self::Local(inner) => Pin::new(inner).poll_flush(cx),
            Self::PCloud(inner) => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        let this = self.get_mut();

        match this {
            Self::Http(inner) | Self::Noop(inner) => Pin::new(inner).poll_shutdown(cx),
            Self::Local(inner) => Pin::new(inner).poll_shutdown(cx),
            Self::PCloud(inner) => Pin::new(inner).poll_shutdown(cx),
        }
    }
}

impl crate::StoreFileWriter for AnyStoreFileWriter {}

#[derive(Debug, derive_more::From)]
#[non_exhaustive]
pub enum AnyStoreDirectory {
    Http(crate::http::HttpStoreDirectory),
    Local(crate::local::LocalStoreDirectory),
    Noop(crate::noop::NoopStoreDirectory),
    PCloud(crate::pcloud::PCloudStoreDirectory),
}

impl crate::StoreDirectory for AnyStoreDirectory {
    type Entry = AnyStoreEntry;
    type Reader = AnyStoreDirectoryReader;

    fn path(&self) -> &std::path::Path {
        match self {
            Self::Http(inner) => inner.path(),
            Self::Local(inner) => inner.path(),
            Self::Noop(inner) => inner.path(),
            Self::PCloud(inner) => inner.path(),
        }
    }

    async fn exists(&self) -> Result<bool> {
        match self {
            Self::Http(inner) => inner.exists().await,
            Self::Local(inner) => inner.exists().await,
            Self::Noop(inner) => inner.exists().await,
            Self::PCloud(inner) => inner.exists().await,
        }
    }

    async fn read(&self) -> Result<Self::Reader> {
        match self {
            Self::Http(inner) => inner.read().await.map(AnyStoreDirectoryReader::Http),
            Self::Local(inner) => inner.read().await.map(AnyStoreDirectoryReader::Local),
            Self::Noop(inner) => inner.read().await.map(AnyStoreDirectoryReader::Noop),
            Self::PCloud(inner) => inner.read().await.map(AnyStoreDirectoryReader::PCloud),
        }
    }

    async fn delete(&self) -> Result<()> {
        match self {
            Self::Http(inner) => inner.delete().await,
            Self::Local(inner) => inner.delete().await,
            Self::Noop(inner) => inner.delete().await,
            Self::PCloud(inner) => inner.delete().await,
        }
    }

    async fn delete_recursive(&self) -> Result<()> {
        match self {
            Self::Http(inner) => inner.delete_recursive().await,
            Self::Local(inner) => inner.delete_recursive().await,
            Self::Noop(inner) => inner.delete_recursive().await,
            Self::PCloud(inner) => inner.delete_recursive().await,
        }
    }
}

/// Type alias for entries in the store, which can be files or directories.
pub type AnyStoreEntry = crate::Entry<AnyStoreFile, AnyStoreDirectory>;

impl From<crate::http::HttpStoreEntry> for AnyStoreEntry {
    fn from(value: crate::http::HttpStoreEntry) -> Self {
        match value {
            crate::Entry::File(file) => crate::Entry::File(file.into()),
            crate::Entry::Directory(directory) => crate::Entry::Directory(directory.into()),
        }
    }
}

impl From<crate::local::LocalStoreEntry> for AnyStoreEntry {
    fn from(value: crate::local::LocalStoreEntry) -> Self {
        match value {
            crate::Entry::File(file) => crate::Entry::File(file.into()),
            crate::Entry::Directory(directory) => crate::Entry::Directory(directory.into()),
        }
    }
}

impl From<crate::noop::NoopStoreEntry> for AnyStoreEntry {
    fn from(value: crate::noop::NoopStoreEntry) -> Self {
        match value {
            crate::Entry::File(file) => crate::Entry::File(file.into()),
            crate::Entry::Directory(directory) => crate::Entry::Directory(directory.into()),
        }
    }
}

impl From<crate::pcloud::PCloudStoreEntry> for AnyStoreEntry {
    fn from(value: crate::pcloud::PCloudStoreEntry) -> Self {
        match value {
            crate::Entry::File(file) => crate::Entry::File(file.into()),
            crate::Entry::Directory(directory) => crate::Entry::Directory(directory.into()),
        }
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum AnyStoreDirectoryReader {
    Http(crate::http::HttpStoreDirectoryReader),
    Local(crate::local::LocalStoreDirectoryReader),
    Noop(crate::noop::NoopStoreDirectoryReader),
    PCloud(crate::pcloud::PCloudStoreDirectoryReader),
}

fn from_poll_entry<E: Into<AnyStoreEntry>>(
    item: Poll<Option<Result<E>>>,
) -> Poll<Option<Result<AnyStoreEntry>>> {
    match item {
        Poll::Ready(Some(Ok(inner))) => Poll::Ready(Some(Ok(inner.into()))),
        Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
        Poll::Ready(None) => Poll::Ready(None),
        Poll::Pending => Poll::Pending,
    }
}

impl futures::Stream for AnyStoreDirectoryReader {
    type Item = Result<AnyStoreEntry>;

    /// Polls for the next directory entry.
    ///
    /// This function is used to asynchronously retrieve the next entry in the
    /// directory.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            Self::Http(inner) => from_poll_entry(inner.poll_next_unpin(cx)),
            Self::Local(inner) => from_poll_entry(inner.poll_next_unpin(cx)),
            Self::Noop(inner) => from_poll_entry(inner.poll_next_unpin(cx)),
            Self::PCloud(inner) => from_poll_entry(inner.poll_next_unpin(cx)),
        }
    }
}

impl crate::StoreDirectoryReader<AnyStoreEntry> for AnyStoreDirectoryReader {}
