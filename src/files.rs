use std::fs::File;
use std::path::Path;

use error::*;

pub fn find_file_relative(root_dir: &Path, uri: &Path) -> HpptResult<Option<File>> {
    let full_path = root_dir.join(uri);

    debug!("{:?} requested, seeing if it exists in root directory ({:?})...", &full_path, root_dir);

    // NOTE: this is subject to race conditions, unfortunately.
    // would need to handle this logic purely through the io::Error type to avoid (TODO?)
    if full_path.exists() {
        if full_path.is_file() {
            debug!("{:?} found, returning.", &full_path);
            Ok(Some(try!(File::open(full_path))))
        } else {
            debug!("{:?} found, but is not a file.", &full_path);
            Ok(None)
        }
    } else {
        debug!("{:?} not found", &full_path);
        Ok(None)
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
            .unwrap()
            .unwrap();
    }

    #[test]
    fn fail_find_file() {

        let f = find_file_relative(&PathBuf::from(env!("CARGO_MANIFEST_DIR")),
                                   &PathBuf::from("DOES_NOT_EXIST"))
            .unwrap();

        assert!(f.is_none());
    }
}
