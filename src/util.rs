use std::io::{Error, ErrorKind, Result};
use std::path::{Component, Path, PathBuf};

pub fn merge_path(root: &Path, child: &Path) -> Result<PathBuf> {
    let mut res = Vec::with_capacity(child.components().count());
    for item in child.components() {
        match item {
            Component::CurDir | Component::RootDir => {}
            Component::Prefix(_prefix) => {
                return Err(Error::other("incompatible path containing prefix"));
            }
            Component::Normal(inner) => {
                res.push(inner);
            }
            Component::ParentDir => {
                if res.pop().is_none() {
                    return Err(Error::new(ErrorKind::NotFound, "No such file or directory"));
                }
            }
        }
    }
    Ok(res
        .into_iter()
        .fold(root.to_path_buf(), |acc, name| acc.join(name)))
}
