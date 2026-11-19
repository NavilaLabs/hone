//! Writing files below the project root, which is untrusted like every checked directory: no
//! symbolic link below the root is followed.

use std::path::Path;

fn outside_root() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "the directory leads outside the project root",
    )
}

/// Replaces the file `path` below `root` with `bytes` through a new file renamed over it, so a
/// link planted at `path` is replaced, not written through.
pub(crate) fn replace(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
    imp::replace(path, root, bytes)
}

/// Creates the file `path` below `root` with `bytes`, and the directories above it that are
/// missing. Fails with `AlreadyExists` if the file exists.
pub(crate) fn create_new(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
    imp::create_new(path, root, bytes)
}

/// Removes the file `path` below `root`.
pub(crate) fn remove(path: &Path, root: &Path) -> std::io::Result<()> {
    imp::remove(path, root)
}

// Every directory below the root is opened through the one above it without following a
// symbolic link, and files are created, renamed, and removed through that handle, so a writer who
// swaps a directory for a link meanwhile cannot redirect them.
#[cfg(unix)]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::os::fd::OwnedFd;
    use std::path::{Component, Path};

    use rustix::fs::{AtFlags, CWD, Mode, OFlags, mkdirat, openat, renameat, unlinkat};
    use rustix::io::Errno;

    use super::outside_root;

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::CLOEXEC);

    fn parent_of<'p>(
        path: &'p Path,
        root: &Path,
        create: bool,
    ) -> std::io::Result<(OwnedFd, &'p OsStr)> {
        let relative = path.strip_prefix(root).map_err(|_| outside_root())?;
        let name = relative.file_name().ok_or_else(outside_root)?;
        let mut directory = openat(CWD, root, DIRECTORY, Mode::empty())?;
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let Component::Normal(part) = component else {
                return Err(outside_root());
            };
            if create {
                match mkdirat(&directory, part, Mode::from_raw_mode(0o777)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            directory = openat(
                &directory,
                part,
                DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| {
                if error == Errno::LOOP || error == Errno::NOTDIR {
                    outside_root()
                } else {
                    error.into()
                }
            })?;
        }
        Ok((directory, name))
    }

    // Removes the new file again if writing it fails, and only then, so that an existing file or
    // a planted link of that name is never touched.
    fn write_new(directory: &OwnedFd, name: &OsStr, bytes: &[u8]) -> std::io::Result<()> {
        let created = openat(
            directory,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        )?;
        let mut file = File::from(created);
        let written = std::io::Write::write_all(&mut file, bytes).and_then(|()| file.sync_all());
        if written.is_err() {
            let _ = unlinkat(directory, name, AtFlags::empty());
        }
        written
    }

    pub(super) fn replace(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let (directory, name) = parent_of(path, root, false)?;
        let mut temporary = OsString::from(".");
        temporary.push(name);
        temporary.push(".hone-new");
        write_new(&directory, &temporary, bytes)?;
        let renamed = renameat(&directory, &temporary, &directory, name);
        if renamed.is_err() {
            let _ = unlinkat(&directory, &temporary, AtFlags::empty());
        }
        Ok(renamed?)
    }

    pub(super) fn create_new(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let (directory, name) = parent_of(path, root, true)?;
        write_new(&directory, name, bytes)
    }

    pub(super) fn remove(path: &Path, root: &Path) -> std::io::Result<()> {
        let (directory, name) = parent_of(path, root, false)?;
        Ok(unlinkat(&directory, name, AtFlags::empty())?)
    }
}

// Without directory handles, the parent is checked below the root before a file is written and
// again before the rename; a writer of the project directory can still swap it in between.
#[cfg(not(unix))]
mod imp {
    use std::fs::File;
    use std::path::Path;

    use super::outside_root;
    use crate::log;

    fn checked_parent(path: &Path, root: &Path) -> std::io::Result<std::path::PathBuf> {
        let canonical = path.parent().unwrap_or(root).canonicalize()?;
        if log::below(&canonical, root) {
            Ok(canonical)
        } else {
            Err(outside_root())
        }
    }

    pub(super) fn replace(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let canonical = checked_parent(path, root)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let temporary = path.with_file_name(format!(".{name}.hone-new"));
        let written = (|| {
            let mut file = File::options()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            std::io::Write::write_all(&mut file, bytes)?;
            file.sync_all()?;
            if checked_parent(path, root)? != canonical {
                return Err(outside_root());
            }
            std::fs::rename(&temporary, path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        written
    }

    pub(super) fn create_new(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        checked_parent(path, root)?;
        let mut file = File::options().write(true).create_new(true).open(path)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()
    }

    pub(super) fn remove(path: &Path, root: &Path) -> std::io::Result<()> {
        checked_parent(path, root)?;
        std::fs::remove_file(path)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{create_new, replace};

    #[test]
    fn replace_follows_no_symbolic_link_below_the_root() {
        let root = std::env::temp_dir().join(format!("hone-replace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("real")).expect("creatable");
        std::fs::write(root.join("real/file.md"), "old").expect("writable");
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).expect("linkable");

        let error = replace(&root.join("link/file.md"), &root, b"new")
            .expect_err("a linked directory is refused, even one that stays below the root");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(root.join("real/file.md")).expect("readable"),
            "old"
        );
        assert!(
            !root.join("real/.file.md.hone-new").exists(),
            "no temporary file is left"
        );

        replace(&root.join("real/file.md"), &root, b"new").expect("a plain directory works");
        assert_eq!(
            std::fs::read_to_string(root.join("real/file.md")).expect("readable"),
            "new"
        );
        assert_eq!(
            std::fs::read_dir(root.join("real"))
                .expect("readable")
                .count(),
            1,
            "the temporary file was renamed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_new_makes_directories_but_follows_no_link_and_overwrites_nothing() {
        let root = std::env::temp_dir().join(format!("hone-create-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let outside = std::env::temp_dir().join(format!("hone-create-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).expect("creatable");
        std::fs::create_dir_all(&outside).expect("creatable");
        std::os::unix::fs::symlink(&outside, root.join("design")).expect("linkable");

        let error = create_new(&root.join("design/tickets/tk-1/ticket.yaml"), &root, b"x")
            .expect_err("a linked directory is refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            std::fs::read_dir(&outside).expect("readable").count(),
            0,
            "nothing is created through the link"
        );

        create_new(&root.join("a/b/file.yaml"), &root, b"one").expect("created");
        assert_eq!(
            std::fs::read(root.join("a/b/file.yaml")).expect("readable"),
            b"one"
        );
        let again = create_new(&root.join("a/b/file.yaml"), &root, b"two").expect_err("exists");
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(root.join("a/b/file.yaml")).expect("readable"),
            b"one"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
