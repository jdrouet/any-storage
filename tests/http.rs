use any_storage::http::HttpStore;
use any_storage::{Store, StoreDirectory, StoreFile, StoreMetadata};
use futures::StreamExt;

#[tokio::test]
async fn scan_irrigationtoolbox() -> std::io::Result<()> {
    let base_url = "https://irrigationtoolbox.com/NEH";
    let store = HttpStore::new(base_url)?;
    let root = store.get_dir("/").await?;
    let reader = root.read().await?;
    let entries = reader.collect::<Vec<_>>().await;
    assert_eq!(entries.len(), 46);

    let files = entries
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .filter_map(|entry| entry.as_file())
        .collect::<Vec<_>>();
    assert!(!files.is_empty());

    for file in files {
        assert!(file.filename().is_some());
        let meta = file.metadata().await?;
        assert_ne!(meta.size(), 0);
    }
    Ok(())
}
