use std::ops::{Bound, RangeBounds};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::{io::Result, pin::Pin};

use crate::{Store, StoreFile, StoreFileReader};

#[derive(Debug)]
struct InnerLocalStore {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LocalStore(Arc<InnerLocalStore>);

impl From<PathBuf> for LocalStore {
    fn from(value: PathBuf) -> Self {
        Self(Arc::new(InnerLocalStore { root: value }))
    }
}

impl Store for LocalStore {
    type File = LocalStoreFile;

    async fn get_file<P: Into<PathBuf>>(&self, path: P) -> Result<Self::File> {
        let path = path.into();
        crate::util::merge_path(&self.0.root, &path).map(|path| LocalStoreFile { path })
    }
}

#[derive(Debug)]
pub struct LocalStoreFile {
    path: PathBuf,
}

impl StoreFile for LocalStoreFile {
    type FileReader = LocalStoreFileReader;

    async fn exists(&self) -> Result<bool> {
        tokio::fs::try_exists(&self.path).await
    }

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
}

#[derive(Debug)]
pub struct LocalStoreFileReader {
    file: tokio::fs::File,
    #[allow(unused)]
    start: u64,
    end: Option<u64>,
    position: u64,
}

impl tokio::io::AsyncRead for LocalStoreFileReader {
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
                buf.put_slice(&temp_read_buf.filled());
                this.position += bytes_read as u64;
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl StoreFileReader for LocalStoreFileReader {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::io::AsyncReadExt;

    use crate::Store;

    use super::*;

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
}
