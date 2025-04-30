use std::io::Result;
use std::ops::RangeBounds;
use std::path::PathBuf;

pub mod http;
pub mod local;

pub(crate) mod util;

pub trait Store {
    type File: StoreFile;

    fn get_file<P: Into<PathBuf>>(
        &self,
        path: P,
    ) -> impl Future<Output = std::io::Result<Self::File>>;
}

pub trait StoreFile {
    type FileReader: StoreFileReader;

    fn exists(&self) -> impl Future<Output = Result<bool>>;
    fn read<R: RangeBounds<u64>>(&self, range: R)
    -> impl Future<Output = Result<Self::FileReader>>;
}

pub trait StoreFileReader: tokio::io::AsyncRead {}
