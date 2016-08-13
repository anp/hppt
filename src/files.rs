use std::fs::File;
use std::path::Path;

/// Hide all I/O errors behind an Option. This will mean that any I/O issue will just cause a 404.
/// Could be handled better, but ideally we don't want to expose permissions issues as a 500.
///
/// Also: checks to make sure canonical path matches requested path. This prevents escaping the
/// content directory under most circumstances, but also means symlinks won't work anymore.
pub fn find_file_relative(root_dir: &Path, uri: &Path) -> Option<File> {
    let full_path = root_dir.join(uri);

    debug!("{:?} requested, seeing if it exists in root directory ({:?})...", &full_path, root_dir);

    let canonical = match full_path.canonicalize() {
        Ok(p) => p,
        Err(why) => {
            debug!("Problem canonicalizing path: {:?}", why);
            return None;
        }
    };

    if canonical != full_path {
        info!("Mismatched canonical ({:?}) and provided ({:?})", canonical, full_path);
        return None;
    }

    // NOTE: this is subject to race conditions, unfortunately.
    // would need to handle this logic purely through the io::Error type to avoid (TODO?)
    if full_path.exists() {
        if full_path.is_file() {
            debug!("{:?} found, returning.", &full_path);
            File::open(full_path).ok() // if there's an issue opening the file, just say None
        } else {
            debug!("{:?} found, but is not a file.", &full_path);
            None
        }
    } else {
        debug!("{:?} not found", &full_path);
        None
    }
}

#[cfg(test)]
mod test {
    use super::find_file_relative;

    use std::path::PathBuf;

    #[test]
    fn successful_find_file() {

        find_file_relative(&PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                           &PathBuf::from("Cargo.toml"))
            .unwrap();
    }

    #[test]
    fn fail_find_file() {

        let f = find_file_relative(&PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                                   &PathBuf::from("DOES_NOT_EXIST"));

        assert!(f.is_none());
    }

    #[test]
    fn fail_escape_content_dir() {
        let f = find_file_relative(&PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                                   &PathBuf::from("../../../../../../../../../etc/passwd"));

        assert!(f.is_none());
    }
}
