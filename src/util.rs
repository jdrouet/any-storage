use std::io::{Error, ErrorKind, Result};
use std::path::{Component, Path, PathBuf};

pub fn merge_path(root: &Path, child: &Path) -> Result<PathBuf> {
    let clean = clean_path(child)?;
    Ok(root.join(clean))
}

pub fn clean_path(path: &Path) -> Result<PathBuf> {
    let mut res = PathBuf::new();
    for item in path.components() {
        match item {
            Component::CurDir | Component::RootDir => {}
            Component::Prefix(_prefix) => {
                return Err(Error::other("incompatible path containing prefix"));
            }
            Component::Normal(inner) => {
                res.push(inner);
            }
            Component::ParentDir => {
                if !res.pop() {
                    return Err(Error::new(ErrorKind::NotFound, "No such file or directory"));
                }
            }
        }
    }
    Ok(res)
}
