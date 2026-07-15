use super::{Component, File, OsString, PackError, Path, PathBuf, fs};

pub(super) fn split_rooted_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), PackError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PackError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    if !absolute.is_absolute() {
        return invalid(format!("path is not absolute: {}", absolute.display()));
    }

    let mut anchor = PathBuf::new();
    let mut components = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if components.is_empty() => {
                anchor.push(component.as_os_str());
            }
            Component::Normal(component) => components.push(component.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                return invalid(format!(
                    "parent traversal rejected in path: {}",
                    path.display()
                ));
            }
            Component::Prefix(_) | Component::RootDir => {
                return invalid(format!("unexpected path root: {}", path.display()));
            }
        }
    }
    if anchor.as_os_str().is_empty() {
        return invalid(format!("path has no filesystem root: {}", path.display()));
    }
    Ok((anchor, components))
}

pub(super) fn open_directory_chain(
    anchor: &Path,
    components: &[OsString],
) -> Result<File, PackError> {
    use cap_primitives::fs::{open_ambient_dir, open_dir_nofollow};

    let mut current = anchor.to_path_buf();
    let mut directory =
        open_ambient_dir(anchor, cap_primitives::ambient_authority()).map_err(|source| {
            PackError::Io {
                path: current.clone(),
                source,
            }
        })?;
    for component in components {
        current.push(component);
        directory = open_dir_nofollow(&directory, Path::new(component)).map_err(|source| {
            PackError::Io {
                path: current.clone(),
                source,
            }
        })?;
    }
    Ok(directory)
}

pub(super) fn open_rooted_directory(path: &Path) -> Result<File, PackError> {
    let (anchor, components) = split_rooted_path(path)?;
    open_directory_chain(&anchor, &components)
}

pub(super) fn open_rooted_regular_file(path: &Path) -> Result<File, PackError> {
    use cap_primitives::fs::{FollowSymlinks, OpenOptions, open};

    let (anchor, mut components) = split_rooted_path(path)?;
    let file_name = components
        .pop()
        .ok_or_else(|| PackError::Invalid(format!("path has no file name: {}", path.display())))?;
    let directory = open_directory_chain(&anchor, &components)?;
    let mut options = OpenOptions::new();
    options.read(true)._cap_fs_ext_follow(FollowSymlinks::No);
    let file =
        open(&directory, Path::new(&file_name), &options).map_err(|source| PackError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    validate_opened_regular_file(file, path)
}

pub(super) fn ensure_safe_absolute_components(path: &Path) -> Result<(), PackError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PackError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|source| PackError::Io {
            path: current.clone(),
            source,
        })?;
        if is_reparse_or_symlink(&metadata) {
            return invalid(format!(
                "symlink/reparse path component: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn open_regular_file(path: &Path) -> Result<File, PackError> {
    open_rooted_regular_file(path)
}

pub(super) fn validate_opened_regular_file(file: File, path: &Path) -> Result<File, PackError> {
    let metadata = file.metadata().map_err(|source| PackError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
        return invalid(format!("asset is not a regular file: {}", path.display()));
    }
    Ok(file)
}

#[cfg(windows)]
pub(super) fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(super) fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub(super) fn require_nonempty(field: &str, value: &str) -> Result<(), PackError> {
    if value.trim().is_empty() {
        return invalid(format!("{field} must not be empty"));
    }
    Ok(())
}

pub(super) fn invalid<T>(message: impl Into<String>) -> Result<T, PackError> {
    Err(PackError::Invalid(message.into()))
}

pub(super) fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;
    let bytes = digest.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing into a String cannot fail");
    }
    output
}
