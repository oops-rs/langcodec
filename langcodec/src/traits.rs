//! Traits for format-agnostic parsing and serialization in langcodec.

use std::{
    env,
    fs::{self, File, Metadata, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Cursor, Write},
    path::{Path, PathBuf},
};

use crate::error::Error;
use tempfile::Builder;

struct WriteTarget {
    path: PathBuf,
    metadata: Option<Metadata>,
}

impl WriteTarget {
    fn resolve(path: &Path) -> Result<Self, Error> {
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            env::current_dir().map_err(Error::Io)?.join(path)
        };
        let parent = absolute_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                Error::InvalidResource("destination path has no parent directory".to_string())
            })?;

        fs::create_dir_all(parent).map_err(map_path_io_error)?;

        match fs::symlink_metadata(&absolute_path) {
            Ok(metadata) => Self::existing(absolute_path, metadata.file_type().is_symlink()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let canonical_parent = fs::canonicalize(parent).map_err(map_path_io_error)?;
                let file_name = absolute_path.file_name().ok_or_else(|| {
                    Error::InvalidResource("destination path does not identify a file".to_string())
                })?;
                Ok(Self {
                    path: canonical_parent.join(file_name),
                    metadata: None,
                })
            }
            Err(error) => Err(map_path_io_error(error)),
        }
    }

    fn existing(path: PathBuf, entered_via_symlink: bool) -> Result<Self, Error> {
        let resolved_path = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) if entered_via_symlink && error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::InvalidResource(
                    "destination symlink is dangling".to_string(),
                ));
            }
            Err(error) => return Err(map_path_io_error(error)),
        };
        let metadata = fs::metadata(&resolved_path).map_err(map_path_io_error)?;

        if !metadata.file_type().is_file() {
            return Err(Error::InvalidResource(format!(
                "destination must resolve to a regular file, but `{}` does not",
                resolved_path.display()
            )));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            if metadata.nlink() > 1 {
                return Err(Error::PolicyViolation(format!(
                    "refusing to replace hard-linked destination `{}` (link count: {})",
                    resolved_path.display(),
                    metadata.nlink()
                )));
            }
        }

        if metadata.permissions().readonly() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "existing destination is read-only",
            )));
        }

        let write_probe = OpenOptions::new()
            .write(true)
            .open(&resolved_path)
            .map_err(Error::Io)?;
        drop(write_probe);

        Ok(Self {
            path: resolved_path,
            metadata: Some(metadata),
        })
    }

    fn parent(&self) -> Result<&Path, Error> {
        self.path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                Error::InvalidResource("destination path has no parent directory".to_string())
            })
    }

    fn create_temporary(
        &self,
    ) -> Result<(tempfile::NamedTempFile, Option<fs::Permissions>), Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(metadata) = &self.metadata {
                let temporary = Builder::new()
                    .tempfile_in(self.parent()?)
                    .map_err(Error::Io)?;
                return Ok((temporary, Some(metadata.permissions())));
            }

            let temporary = {
                let mut builder = Builder::new();
                builder.permissions(fs::Permissions::from_mode(0o666));
                builder.tempfile_in(self.parent()?).map_err(Error::Io)?
            };
            let final_permissions = temporary
                .as_file()
                .metadata()
                .map_err(Error::Io)?
                .permissions();
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(Error::Io)?;

            Ok((temporary, Some(final_permissions)))
        }

        #[cfg(not(unix))]
        {
            let temporary = Builder::new()
                .tempfile_in(self.parent()?)
                .map_err(Error::Io)?;
            let final_permissions = self
                .metadata
                .as_ref()
                .map(|metadata| metadata.permissions());
            Ok((temporary, final_permissions))
        }
    }

    fn persist(&self, temporary: tempfile::NamedTempFile) -> Result<(), Error> {
        let persisted_file = if self.metadata.is_some() {
            temporary.persist(&self.path)
        } else {
            temporary.persist_noclobber(&self.path)
        }
        .map_err(|error| Error::Io(error.error))?;
        drop(persisted_file);

        #[cfg(unix)]
        File::open(self.parent()?)
            .and_then(|directory| directory.sync_all())
            .map_err(Error::Io)?;

        Ok(())
    }
}

fn map_path_io_error(error: std::io::Error) -> Error {
    #[cfg(unix)]
    if error.raw_os_error() == Some(libc::ELOOP) {
        Error::PolicyViolation("destination path contains an unsafe symlink loop".to_string())
    } else {
        Error::Io(error)
    }

    #[cfg(not(unix))]
    Error::Io(error)
}

/// A trait for parsing and writing localization resources from/to one file.
///
/// # Example
///
/// ```rust,no_run
/// use langcodec::traits::Parser;
/// let format = langcodec::formats::strings::Format::read_from("en.strings")?;
/// format.write_to("en_copy.strings")?;
/// Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait Parser {
    /// Parse from any reader.
    fn from_reader<R: BufRead>(reader: R) -> Result<Self, Error>
    where
        Self: Sized;

    /// Parse from file path.
    fn read_from<P: AsRef<Path>>(path: P) -> Result<Self, Error>
    where
        Self: Sized,
    {
        let path = path.as_ref();
        let result = (|| {
            let file = File::open(path).map_err(Error::Io)?;
            let reader = BufReader::new(file);
            Self::from_reader(reader)
        })();

        result.map_err(|error| error.with_path(path))
    }

    /// Write to any writer (file, memory, etc.).
    fn to_writer<W: Write>(&self, writer: W) -> Result<(), Error>;

    /// Write to file path.
    ///
    /// The serialized data is flushed to a temporary file in the destination
    /// directory before atomically replacing the destination. Symlinks are
    /// resolved and the referent is replaced, leaving the symlink itself
    /// intact. On Unix, content remains owner-only until serialization and
    /// flushing succeed, then the final mode is applied. Existing Unix mode
    /// bits are retained; ownership, ACLs, extended attributes, and other
    /// platform metadata are not copied.
    fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let display_path = path.as_ref();
        let result = (|| {
            let target = WriteTarget::resolve(display_path)?;
            let (mut temporary, final_permissions) = target.create_temporary()?;
            {
                let mut writer = BufWriter::new(temporary.as_file_mut());
                self.to_writer(&mut writer)?;
                writer.flush().map_err(Error::Io)?;
            }

            if let Some(final_permissions) = final_permissions {
                temporary
                    .as_file()
                    .set_permissions(final_permissions)
                    .map_err(Error::Io)?;
            }
            temporary.as_file().sync_all().map_err(Error::Io)?;

            target.persist(temporary)
        })();

        result.map_err(|error| error.with_path(display_path))
    }

    /// Parse from a string.
    fn from_str(s: &str) -> Result<Self, Error>
    where
        Self: Sized,
    {
        Self::from_reader(Cursor::new(s))
    }

    /// Parse from bytes.
    fn from_bytes(bytes: &[u8]) -> Result<Self, Error>
    where
        Self: Sized,
    {
        Self::from_reader(Cursor::new(bytes))
    }
}
