use std::borrow::Cow;
use std::io::{Error, ErrorKind, Result, SeekFrom};
use std::ops::{Bound, RangeBounds};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::SystemTime;

use futures::Stream;
use tokio::io::AsyncSeekExt;

use crate::{Entry, Store, StoreDirectory, StoreFile, StoreFileReader, WriteMode};

/// Internal representation of the local store with a root path.
#[derive(Debug)]
struct InnerLocalStore {
    root: PathBuf,
}

/// Wrapper for the local store, enabling shared ownership.
#[derive(Debug, Clone)]
pub struct LocalStore(Arc<InnerLocalStore>);

impl LocalStore {
    /// Constructor of the localstore
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self::from(path.into())
    }
}

impl From<PathBuf> for LocalStore {
    /// Converts a `PathBuf` into a `LocalStore`.
    ///
    /// Takes the root path of the local store and wraps it in an `Arc`.
    fn from(value: PathBuf) -> Self {
        Self(Arc::new(InnerLocalStore { root: value }))
    }
}

impl Store for LocalStore {
    type Directory = LocalStoreDirectory;
    type File = LocalStoreFile;

    /// Retrieves a directory at the specified path in the local store.
    ///
    /// Merges the root path with the given path to obtain the full directory
    /// path.
    async fn get_dir<P: Into<PathBuf>>(&self, path: P) -> Result<Self::Directory> {
        let path = path.into();
        crate::util::merge_path(&self.0.root, &path).map(|path| LocalStoreDirectory { path })
    }

    /// Retrieves a file at the specified path in the local store.
    ///
    /// Merges the root path with the given path to obtain the full file path.
    async fn get_file<P: Into<PathBuf>>(&self, path: P) -> Result<Self::File> {
        let path = path.into();
        crate::util::merge_path(&self.0.root, &path).map(|path| LocalStoreFile { path })
    }
}

/// Type alias for entries in the local store, which can be files or
/// directories.
pub type LocalStoreEntry = Entry<LocalStoreFile, LocalStoreDirectory>;

impl LocalStoreEntry {
    /// Creates a new `LocalStoreEntry` from a `tokio::fs::DirEntry`.
    ///
    /// The entry is classified as either a file or directory based on its path.
    pub fn new(entry: tokio::fs::DirEntry) -> Result<Self> {
        let path = entry.path();
        if path.is_dir() {
            Ok(Self::Directory(LocalStoreDirectory { path }))
        } else if path.is_file() {
            Ok(Self::File(LocalStoreFile { path }))
        } else {
            Err(Error::new(
                ErrorKind::Unsupported,
                "expected a file or a directory",
            ))
        }
    }
}

/// Representation of a directory in the local store.
#[derive(Debug)]
pub struct LocalStoreDirectory {
    path: PathBuf,
}

impl StoreDirectory for LocalStoreDirectory {
    type Entry = LocalStoreEntry;
    type Reader = LocalStoreDirectoryReader;

    /// Checks if the directory exists.
    ///
    /// Returns a future that resolves to `true` if the directory exists,
    /// otherwise `false`.
    async fn exists(&self) -> Result<bool> {
        tokio::fs::try_exists(&self.path).await
    }

    /// Reads the contents of the directory.
    ///
    /// Returns a future that resolves to a reader for iterating over the
    /// directory's entries.
    async fn read(&self) -> Result<Self::Reader> {
        tokio::fs::read_dir(&self.path)
            .await
            .map(|value| LocalStoreDirectoryReader {
                inner: Box::pin(value),
            })
    }
}

/// Reader for streaming entries from a local store directory.
#[derive(Debug)]
pub struct LocalStoreDirectoryReader {
    inner: Pin<Box<tokio::fs::ReadDir>>,
}

impl Stream for LocalStoreDirectoryReader {
    type Item = Result<LocalStoreEntry>;

    /// Polls for the next directory entry.
    ///
    /// This function is used to asynchronously retrieve the next entry in the
    /// directory.
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut inner = self.get_mut().inner.as_mut();

        match inner.poll_next_entry(cx) {
            Poll::Ready(Ok(Some(entry))) => Poll::Ready(Some(LocalStoreEntry::new(entry))),
            Poll::Ready(Ok(None)) => Poll::Ready(None),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl crate::StoreDirectoryReader<LocalStoreEntry> for LocalStoreDirectoryReader {}

/// Representation of a file in the local store.
#[derive(Debug)]
pub struct LocalStoreFile {
    path: PathBuf,
}

impl StoreFile for LocalStoreFile {
    type FileReader = LocalStoreFileReader;
    type FileWriter = LocalStoreFileWriter;
    type Metadata = LocalStoreFileMetadata;

    /// Retrieves the file name from the path.
    ///
    /// This function extracts the file name by iterating over the components of
    /// the path in reverse order.
    fn filename(&self) -> Option<Cow<'_, str>> {
        self.path
            .components()
            .rev()
            .filter_map(|item| match item {
                Component::Normal(inner) => Some(inner),
                _ => None,
            })
            .next()
            .map(|value| value.to_string_lossy())
    }

    /// Checks if the file exists.
    ///
    /// Returns a future that resolves to `true` if the file exists, otherwise
    /// `false`.
    async fn exists(&self) -> Result<bool> {
        tokio::fs::try_exists(&self.path).await
    }

    /// Retrieves the metadata of the file.
    ///
    /// Returns a future that resolves to the file's metadata, such as size and
    /// timestamps.
    async fn metadata(&self) -> Result<Self::Metadata> {
        let meta = tokio::fs::metadata(&self.path).await?;
        let size = meta.size();
        let created = meta
            .created()
            .ok()
            .and_then(|v| v.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let modified = meta
            .modified()
            .ok()
            .and_then(|v| v.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(LocalStoreFileMetadata {
            size,
            created,
            modified,
        })
    }

    /// Reads a portion of the file's content, specified by a byte range.
    ///
    /// Returns a future that resolves to a reader that can read the specified
    /// range of the file.
    async fn read<R: RangeBounds<u64>>(&self, range: R) -> Result<Self::FileReader> {
        use tokio::io::AsyncSeekExt;

        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&n) => Some(n + 1),
            Bound::Excluded(&n) => Some(n),
            Bound::Unbounded => None, // no limit
        };

        let mut file = tokio::fs::OpenOptions::new()
            .read(true)
            .open(&self.path)
            .await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        Ok(LocalStoreFileReader {
            file,
            start,
            end,
            position: start,
        })
    }

    async fn write(&self, options: crate::WriteOptions) -> Result<Self::FileWriter> {
        let mut file = tokio::fs::OpenOptions::new()
            .append(matches!(options.mode, WriteMode::Append))
            .truncate(matches!(options.mode, WriteMode::Truncate { .. }))
            .write(true)
            .create(true)
            .open(&self.path)
            .await?;
        match options.mode {
            WriteMode::Truncate { offset } if offset > 0 => {
                file.seek(SeekFrom::Start(offset)).await?;
            }
            _ => {}
        };
        Ok(LocalStoreFileWriter(file))
    }
}

/// Metadata associated with a file in the local store (size, created, modified
/// timestamps).
#[derive(Clone, Debug)]
pub struct LocalStoreFileMetadata {
    size: u64,
    created: u64,
    modified: u64,
}

impl super::StoreMetadata for LocalStoreFileMetadata {
    /// Returns the size of the file in bytes.
    fn size(&self) -> u64 {
        self.size
    }

    /// Returns the creation timestamp of the file (epoch time).
    fn created(&self) -> u64 {
        self.created
    }

    /// Returns the last modification timestamp of the file (epoch time).
    fn modified(&self) -> u64 {
        self.modified
    }
}

/// Reader for asynchronously reading the contents of a file in the local store.
#[derive(Debug)]
pub struct LocalStoreFileReader {
    file: tokio::fs::File,
    #[allow(unused)]
    start: u64,
    end: Option<u64>,
    position: u64,
}

impl tokio::io::AsyncRead for LocalStoreFileReader {
    /// Polls for reading data from the file.
    ///
    /// This function reads data into the provided buffer, handling partial
    /// reads within the given range.
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let remaining = match self.end {
            Some(end) => end.saturating_sub(self.position) as usize,
            None => buf.remaining(),
        };

        if remaining == 0 {
            return std::task::Poll::Ready(Ok(()));
        }

        // Limit the read buffer to the remaining range
        let read_len = std::cmp::min(remaining, buf.remaining()) as usize;
        let mut temp_buf = vec![0u8; read_len];
        let mut temp_read_buf = tokio::io::ReadBuf::new(&mut temp_buf);

        let this = self.as_mut().get_mut();
        let pinned_file = Pin::new(&mut this.file);

        match pinned_file.poll_read(cx, &mut temp_read_buf) {
            Poll::Ready(Ok(())) => {
                let bytes_read = temp_read_buf.filled().len();
                buf.put_slice(temp_read_buf.filled());
                this.position += bytes_read as u64;
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl StoreFileReader for LocalStoreFileReader {}

#[derive(Debug)]
pub struct LocalStoreFileWriter(tokio::fs::File);

impl tokio::io::AsyncWrite for LocalStoreFileWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        // Pinning the inner file (unwrap since it is wrapped in Pin)
        let file = &mut self.as_mut().0;

        // Use tokio::io::AsyncWriteExt::write to write to the file
        Pin::new(file).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        let file = &mut self.as_mut().0;
        Pin::new(file).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<()>> {
        let file = &mut self.as_mut().0;
        Pin::new(file).poll_shutdown(cx)
    }
}

impl crate::StoreFileWriter for LocalStoreFileWriter {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::io::AsyncReadExt;

    use super::*;
    use crate::Store;

    #[tokio::test]
    async fn should_not_go_in_parent_folder() {
        let current = PathBuf::from(env!("PWD"));
        let store = LocalStore::from(current);

        let _ = store.get_file("anywhere/../hello.txt").await.unwrap();

        let err = store.get_file("../hello.txt").await.unwrap_err();
        assert_eq!(err.to_string(), "No such file or directory");
    }

    #[tokio::test]
    async fn should_find_existing_files() {
        let current = PathBuf::from(env!("PWD"));
        let store = LocalStore::from(current);

        let lib = store.get_file("/src/lib.rs").await.unwrap();
        assert!(lib.exists().await.unwrap());

        let lib = store.get_file("src/lib.rs").await.unwrap();
        assert!(lib.exists().await.unwrap());

        let lib = store.get_file("nothing/../src/lib.rs").await.unwrap();
        assert!(lib.exists().await.unwrap());

        let missing = store.get_file("nothing.rs").await.unwrap();
        assert!(!missing.exists().await.unwrap());
    }

    #[tokio::test]
    async fn should_read_lib_file() {
        let current = PathBuf::from(env!("PWD"));
        let store = LocalStore::from(current);

        let lib = store.get_file("/src/lib.rs").await.unwrap();
        let mut reader = lib.read(0..10).await.unwrap();
        let mut buffer = vec![];
        reader.read_to_end(&mut buffer).await.unwrap();

        let content = include_bytes!("./lib.rs");
        assert_eq!(buffer, content[0..10]);
    }

    #[tokio::test]
    async fn should_read_lib_metadata() {
        let current = PathBuf::from(env!("PWD"));
        let store = LocalStore::from(current);

        let lib = store.get_file("/src/lib.rs").await.unwrap();
        let meta = lib.metadata().await.unwrap();

        assert!(meta.size > 0);
        assert!(meta.created > 0);
        assert!(meta.modified > 0);
    }
}
