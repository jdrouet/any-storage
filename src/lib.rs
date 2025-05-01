use std::borrow::Cow;
use std::io::Result;
use std::ops::RangeBounds;
use std::path::PathBuf;

use futures::Stream;

pub mod http;
pub mod local;

pub(crate) mod util;

pub trait Store {
    type Directory: StoreDirectory;
    type File: StoreFile;

    fn get_dir<P: Into<PathBuf>>(&self, path: P) -> impl Future<Output = Result<Self::Directory>>;
    fn get_file<P: Into<PathBuf>>(&self, path: P) -> impl Future<Output = Result<Self::File>>;
}

pub trait StoreDirectory {
    type Entry;
    type Reader: StoreDirectoryReader<Self::Entry>;

    fn exists(&self) -> impl Future<Output = Result<bool>>;
    fn read(&self) -> impl Future<Output = Result<Self::Reader>>;
}

pub trait StoreDirectoryReader<E>: Stream<Item = Result<E>> + Sized {}

pub trait StoreFile {
    type FileReader: StoreFileReader;
    type Metadata: StoreMetadata;

    fn filename(&self) -> Option<Cow<'_, str>>;
    fn exists(&self) -> impl Future<Output = Result<bool>>;
    fn metadata(&self) -> impl Future<Output = Result<Self::Metadata>>;
    fn read<R: RangeBounds<u64>>(&self, range: R)
    -> impl Future<Output = Result<Self::FileReader>>;
}

pub trait StoreFileReader: tokio::io::AsyncRead {}

#[derive(Debug)]
pub enum Entry<File, Directory> {
    File(File),
    Directory(Directory),
}

impl<File, Directory> Entry<File, Directory> {
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    pub fn as_directory(&self) -> Option<&Directory> {
        match self {
            Self::Directory(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_file(&self) -> Option<&File> {
        match self {
            Self::File(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn into_directory(self) -> std::result::Result<Directory, Self> {
        match self {
            Self::Directory(inner) => Ok(inner),
            other => Err(other),
        }
    }

    pub fn into_file(self) -> std::result::Result<File, Self> {
        match self {
            Self::File(inner) => Ok(inner),
            other => Err(other),
        }
    }
}

pub trait StoreMetadata {
    fn size(&self) -> u64;
    fn created(&self) -> u64;
    fn modified(&self) -> u64;
}
