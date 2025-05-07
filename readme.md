# Virtual FileStore Abstraction for different Backends

This crate provides a unified async abstraction over multiple file storage backends,
enabling consistent access to files and directories whether they are located on the
local filesystem, an HTTP file index, or any other cloud storage service.

## ✨ Features

- **Unified `Store` Trait**: A common trait for file and directory access across storage backends.
- **Streaming Reads and Writes**: All file reads and writes are implemented using `tokio::io::AsyncRead` and `tokio::io::AsyncWrite` for efficient streaming.
- **Range Requests**: HTTP and pCloud support partial reads using byte-range headers.
- **Metadata Support**: Each backend provides basic file metadata such as size, creation, and modification time.
- **Pluggable Backends**: Easily extendable to support additional services like S3, FTP, or Dropbox.

## 📦 Available Backends

### ✅ `LocalStore`
- Accesses files and directories directly from the local filesystem.

### 🌐 `HttpStore`
- Reads from public HTTP directories using HTML parsing (for Apache/Nginx-style listings).

### ☁️ `PCloudStore`
- Authenticates using username/password and reads from a pCloud account.

## 🔧 Usage Example

```rust,ignore
use use_storage::{Store, LocalStore};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let store = LocalStore::new("/some/base/path");
    let file = store.get_file("file.txt").await?;
    let mut reader = file.read(..).await?;
    tokio::io::copy(&mut reader, &mut tokio::io::stdout()).await?;
    Ok(())
}
```

## 📚 Architecture

- `Store`: Main trait for fetching files and directories.
- `StoreFile` and `StoreDirectory`: Trait abstractions for files and folders.
- `StoreFileReader`: Async stream wrapper over file content.
- `StoreFileWriter`: Async stream wrapper to write to a file.
- Each backend implements these traits for its own types, hiding service-specific logic behind the unified interface.

## 🔐 Security Notes

- Credentials for `PCloudStore` must be managed securely; currently supports username/password only.
- HTTP support is read-only and assumes publicly exposed directory indexes.

## 📦 Crate Status

- Written in Rust 2024 Edition
- Async-first design, built on top of `tokio`, `reqwest`, `futures`, and `bytes`.

## 🚧 TODO / Roadmap

- Delete support
- More backends (S3, google drive, proton drive, etc)
