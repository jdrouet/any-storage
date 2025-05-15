use std::io::{Error, ErrorKind, Result};
use std::ops::RangeBounds;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::Poll;

use futures::Stream;

use crate::{Entry, Store, StoreDirectory, StoreFile, StoreFileReader};

/// Configuration for [`NoopStore`].
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct NoopStoreConfig;

impl NoopStoreConfig {
    pub fn build(&self) -> Result<NoopStore> {
        Ok(NoopStore)
    }
}

/// Noop store, does nothing, can't do anything
#[derive(Clone, Debug, Default)]
pub struct NoopStore;

impl Store for NoopStore {
    type Directory = NoopStoreDirectory;
    type File = NoopStoreFile;

    async fn get_dir<P: Into<PathBuf>>(&self, path: P) -> Result<Self::Directory> {
        let path = path.into();
        Ok(NoopStoreDirectory { path })
    }

    async fn get_file<P: Into<PathBuf>>(&self, path: P) -> Result<Self::File> {
        let path = path.into();
        Ok(NoopStoreFile { path })
    }
}

pub type NoopStoreEntry = Entry<NoopStoreFile, NoopStoreDirectory>;

/// Representation of a directory in the noop store.
#[derive(Debug)]
pub struct NoopStoreDirectory {
    path: PathBuf,
}

impl StoreDirectory for NoopStoreDirectory {
    type Entry = NoopStoreEntry;
    type Reader = NoopStoreDirectoryReader;

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    async fn exists(&self) -> Result<bool> {
        Ok(false)
    }

    async fn read(&self) -> Result<Self::Reader> {
        Err(Error::new(ErrorKind::NotFound, "directory not found"))
    }

    async fn delete(&self) -> Result<()> {
        Err(Error::new(ErrorKind::NotFound, "directory not found"))
    }

    async fn delete_recursive(&self) -> Result<()> {
        Err(Error::new(ErrorKind::NotFound, "directory not found"))
    }
}

/// Reader for streaming entries from a noop store directory.
///
/// Should never be built.
#[derive(Debug)]
pub struct NoopStoreDirectoryReader;

impl Stream for NoopStoreDirectoryReader {
    type Item = Result<NoopStoreEntry>;

    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

impl crate::StoreDirectoryReader<NoopStoreEntry> for NoopStoreDirectoryReader {}

/// Representation of a file in the noop store.
#[derive(Debug)]
pub struct NoopStoreFile {
    path: PathBuf,
}

impl StoreFile for NoopStoreFile {
    type FileReader = NoopStoreFileReader;
    type FileWriter = NoopStoreFileWriter;
    type Metadata = NoopStoreFileMetadata;

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    async fn exists(&self) -> Result<bool> {
        Ok(false)
    }

    async fn metadata(&self) -> Result<Self::Metadata> {
        Err(Error::new(ErrorKind::NotFound, "file not found"))
    }

    async fn read<R: RangeBounds<u64>>(&self, _range: R) -> Result<Self::FileReader> {
        Err(Error::new(ErrorKind::NotFound, "file not found"))
    }

    async fn write(&self, _options: crate::WriteOptions) -> Result<Self::FileWriter> {
        Err(Error::new(ErrorKind::Unsupported, "unable to write"))
    }

    async fn delete(&self) -> Result<()> {
        Err(Error::new(ErrorKind::NotFound, "file not found"))
    }
}

/// Metadata associated with a file in the store store.
///
/// Should never be built.
#[derive(Clone, Debug)]
pub struct NoopStoreFileMetadata;

impl super::StoreMetadata for NoopStoreFileMetadata {
    fn size(&self) -> u64 {
        0
    }

    fn created(&self) -> u64 {
        0
    }

    fn modified(&self) -> u64 {
        0
    }
}

/// Reader for asynchronously reading the contents of a file in the noop store.
///
/// Should never be built.
#[derive(Debug)]
pub struct NoopStoreFileReader;

impl tokio::io::AsyncRead for NoopStoreFileReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(Error::new(ErrorKind::NotFound, "file not found")))
    }
}

impl StoreFileReader for NoopStoreFileReader {}

/// Writer for files in the noop store.
///
/// Should never be built.
#[derive(Debug)]
pub struct NoopStoreFileWriter;

impl tokio::io::AsyncWrite for NoopStoreFileWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &[u8],
    ) -> Poll<Result<usize>> {
        Poll::Ready(Err(Error::new(ErrorKind::Unsupported, "unable to write")))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Err(Error::new(ErrorKind::Unsupported, "unable to write")))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Err(Error::new(ErrorKind::Unsupported, "unable to write")))
    }
}

impl crate::StoreFileWriter for NoopStoreFileWriter {}
