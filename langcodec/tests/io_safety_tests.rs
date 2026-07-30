use std::{
    error::Error as StdError,
    io::{BufRead, Write},
};

#[cfg(unix)]
use std::path::PathBuf;

use langcodec::{
    Codec, Error, ErrorCode, Metadata, Resource,
    converter::convert_resources_to_format,
    formats::{FormatType, strings::Format as StringsFormat},
    traits::Parser,
};
use tempfile::tempdir;

#[derive(Debug)]
struct ReadFailure;

impl Parser for ReadFailure {
    fn from_reader<R: BufRead>(_reader: R) -> Result<Self, Error> {
        Err(Error::InvalidResource(
            "deliberate read failure".to_string(),
        ))
    }

    fn to_writer<W: Write>(&self, _writer: W) -> Result<(), Error> {
        Ok(())
    }
}

struct PartialWriteFailure;

impl Parser for PartialWriteFailure {
    fn from_reader<R: BufRead>(_reader: R) -> Result<Self, Error> {
        Ok(Self)
    }

    fn to_writer<W: Write>(&self, mut writer: W) -> Result<(), Error> {
        writer.write_all(b"partial").map_err(Error::Io)?;
        Err(Error::Validation(
            "deliberate serializer failure".to_string(),
        ))
    }
}

struct SuccessfulWrite<'a>(&'a [u8]);

impl Parser for SuccessfulWrite<'_> {
    fn from_reader<R: BufRead>(_reader: R) -> Result<Self, Error> {
        Ok(Self(&[]))
    }

    fn to_writer<W: Write>(&self, mut writer: W) -> Result<(), Error> {
        writer.write_all(self.0).map_err(Error::Io)
    }
}

struct MustNotSerialize;

impl Parser for MustNotSerialize {
    fn from_reader<R: BufRead>(_reader: R) -> Result<Self, Error> {
        Ok(Self)
    }

    fn to_writer<W: Write>(&self, _writer: W) -> Result<(), Error> {
        Err(Error::Validation(
            "serializer was invoked before destination preflight completed".to_string(),
        ))
    }
}

#[cfg(unix)]
struct TempModeObservingWrite {
    directory: PathBuf,
    destination: PathBuf,
    expected_mode: u32,
}

#[cfg(unix)]
impl Parser for TempModeObservingWrite {
    fn from_reader<R: BufRead>(_reader: R) -> Result<Self, Error> {
        Ok(Self {
            directory: PathBuf::new(),
            destination: PathBuf::new(),
            expected_mode: 0,
        })
    }

    fn to_writer<W: Write>(&self, mut writer: W) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;

        let mut temporary_paths = Vec::new();
        for entry in std::fs::read_dir(&self.directory).map_err(Error::Io)? {
            let path = entry.map_err(Error::Io)?.path();
            if path != self.destination
                && std::fs::symlink_metadata(&path)
                    .map_err(Error::Io)?
                    .file_type()
                    .is_file()
            {
                temporary_paths.push(path);
            }
        }

        let [temporary_path] = temporary_paths.as_slice() else {
            return Err(Error::Validation(format!(
                "expected one temporary file during serialization, found {}",
                temporary_paths.len()
            )));
        };
        let actual_mode = std::fs::metadata(temporary_path)
            .map_err(Error::Io)?
            .permissions()
            .mode()
            & 0o777;
        if actual_mode != self.expected_mode {
            return Err(Error::Validation(format!(
                "temporary mode was {actual_mode:o}, expected {:o}",
                self.expected_mode
            )));
        }

        writer.write_all(b"replacement").map_err(Error::Io)
    }
}

fn assert_path_context(error: &Error, path: &std::path::Path) {
    assert_eq!(
        error.structured().context.and_then(|context| context.path),
        Some(path.to_string_lossy().into_owned())
    );
}

fn empty_resource() -> Resource {
    Resource {
        metadata: Metadata {
            language: "en".to_string(),
            domain: "tests".to_string(),
            custom: std::collections::HashMap::new(),
        },
        entries: Vec::new(),
    }
}

#[test]
fn read_from_attaches_exact_path_and_retains_original_error() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("broken.input");
    std::fs::write(&path, b"input").expect("write test input");

    let error = ReadFailure::read_from(&path).expect_err("dummy parser must fail");
    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert_path_context(&error, &path);

    assert!(StdError::source(&error).is_some());
    let Error::WithPath {
        path: wrapped_path,
        source,
    } = &error
    else {
        panic!("read error must have a path wrapper");
    };
    assert_eq!(wrapped_path, &path.to_string_lossy());
    assert!(
        matches!(source.as_ref(), Error::InvalidResource(message) if message == "deliberate read failure")
    );
}

#[test]
fn strings_read_from_open_error_includes_display_path() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("missing.strings");

    let error = StringsFormat::read_from(&path).expect_err("missing input must fail");

    assert_eq!(error.error_code(), ErrorCode::Io);
    assert_path_context(&error, &path);
    assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
}

#[test]
fn strings_read_from_malformed_utf8_includes_display_path() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("malformed-utf8.strings");
    std::fs::write(&path, [0xFF]).expect("write malformed UTF-8 input");

    let error = StringsFormat::read_from(&path).expect_err("malformed UTF-8 must fail");

    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert!(error.to_string().contains("Invalid UTF-8"));
    assert_path_context(&error, &path);
}

#[test]
fn strings_read_from_malformed_utf16_includes_display_path() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("malformed-utf16.strings");
    std::fs::write(&path, [0xFF, 0xFE, 0x00]).expect("write malformed UTF-16 input");

    let error = StringsFormat::read_from(&path).expect_err("malformed UTF-16 must fail");

    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert!(error.to_string().contains("Invalid UTF-16LE"));
    assert_path_context(&error, &path);
}

#[test]
fn strings_read_from_valid_utf16_bom_still_decodes() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("valid-utf16.strings");
    let mut encoded = vec![0xFF, 0xFE];
    for code_unit in r#""hello" = "Hello";"#.encode_utf16() {
        encoded.extend_from_slice(&code_unit.to_le_bytes());
    }
    std::fs::write(&path, encoded).expect("write UTF-16 input");

    let format = StringsFormat::read_from(&path).expect("decode valid UTF-16 input");

    assert_eq!(format.pairs.len(), 1);
    assert_eq!(format.pairs[0].key, "hello");
    assert_eq!(format.pairs[0].value, "Hello");
}

#[test]
fn serializer_failure_after_partial_write_preserves_existing_destination() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");
    std::fs::write(&path, b"original").expect("write original destination");

    let error = PartialWriteFailure
        .write_to(&path)
        .expect_err("dummy serializer must fail");

    assert_eq!(error.error_code(), ErrorCode::Validation);
    assert_eq!(
        std::fs::read(&path).expect("read destination after failed write"),
        b"original"
    );
}

#[test]
fn successful_write_creates_parents_and_replaces_destination() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("nested").join("localizations.txt");

    SuccessfulWrite(b"first")
        .write_to(&path)
        .expect("write new destination");
    SuccessfulWrite(b"replacement")
        .write_to(&path)
        .expect("replace destination");

    assert_eq!(
        std::fs::read(&path).expect("read replaced destination"),
        b"replacement"
    );
}

#[cfg(unix)]
#[test]
fn existing_file_temporary_is_restrictive_during_serialization() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");
    std::fs::write(&path, b"original").expect("write original destination");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))
        .expect("set original permissions");

    TempModeObservingWrite {
        directory: directory.path().to_path_buf(),
        destination: path.clone(),
        expected_mode: 0o600,
    }
    .write_to(&path)
    .expect("replace destination after observing temporary mode");

    assert_eq!(
        std::fs::read(&path).expect("read replaced destination"),
        b"replacement"
    );
    let final_mode = std::fs::metadata(&path)
        .expect("read replacement metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(final_mode, 0o640);
}

#[cfg(unix)]
#[test]
fn new_file_temporary_is_restrictive_during_serialization() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");

    TempModeObservingWrite {
        directory: directory.path().to_path_buf(),
        destination: path.clone(),
        expected_mode: 0o600,
    }
    .write_to(&path)
    .expect("write destination after observing temporary mode");

    assert_eq!(
        std::fs::read(&path).expect("read destination"),
        b"replacement"
    );
}

#[cfg(unix)]
#[test]
fn new_file_uses_standard_creation_permissions_after_umask() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("create test directory");
    let reference_path = directory.path().join("reference.txt");
    std::fs::File::create(&reference_path).expect("create reference with standard permissions");
    let expected_mode = std::fs::metadata(&reference_path)
        .expect("read reference metadata")
        .permissions()
        .mode()
        & 0o777;

    let path = directory.path().join("localizations.txt");
    SuccessfulWrite(b"new")
        .write_to(&path)
        .expect("write new destination");
    let actual_mode = std::fs::metadata(&path)
        .expect("read new destination metadata")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(actual_mode, expected_mode);
}

#[cfg(unix)]
#[test]
fn successful_replacement_preserves_existing_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");
    std::fs::write(&path, b"original").expect("write original destination");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))
        .expect("set original permissions");

    SuccessfulWrite(b"replacement")
        .write_to(&path)
        .expect("replace destination");

    let mode = std::fs::metadata(&path)
        .expect("read replacement metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o640);
}

#[cfg(unix)]
#[test]
fn symlink_chain_is_preserved_while_referent_is_atomically_replaced() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("create test directory");
    let referent_directory = directory.path().join("referents");
    std::fs::create_dir(&referent_directory).expect("create referent directory");
    let referent = referent_directory.join("localizations.txt");
    std::fs::write(&referent, b"original").expect("write original referent");

    let first_link = directory.path().join("first-link");
    let second_link = directory.path().join("second-link");
    symlink("referents/localizations.txt", &first_link).expect("create first symlink");
    symlink("first-link", &second_link).expect("create second symlink");

    SuccessfulWrite(b"replacement")
        .write_to(&second_link)
        .expect("replace symlink referent");

    assert!(
        std::fs::symlink_metadata(&first_link)
            .expect("inspect first symlink")
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&second_link)
            .expect("inspect second symlink")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_link(&first_link).expect("read first symlink"),
        std::path::Path::new("referents/localizations.txt")
    );
    assert_eq!(
        std::fs::read_link(&second_link).expect("read second symlink"),
        std::path::Path::new("first-link")
    );
    assert_eq!(
        std::fs::read(&referent).expect("read replaced referent"),
        b"replacement"
    );
}

#[cfg(unix)]
#[test]
fn dangling_symlink_is_rejected_without_replacing_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("dangling-link");
    let nested_directory = directory.path().join("nested");
    std::fs::create_dir(&nested_directory).expect("create nested directory");
    symlink("missing-referent", &path).expect("create dangling symlink");
    let caller_path = nested_directory.join("..").join("dangling-link");

    let error = MustNotSerialize
        .write_to(&caller_path)
        .expect_err("dangling symlink must fail preflight");

    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert!(error.to_string().contains("dangling"));
    assert_path_context(&error, &caller_path);
    assert!(
        std::fs::symlink_metadata(&path)
            .expect("dangling symlink must remain")
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn symlink_loop_is_rejected_as_unsafe() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("create test directory");
    let first_link = directory.path().join("first-link");
    let second_link = directory.path().join("second-link");
    symlink("second-link", &first_link).expect("create first symlink");
    symlink("first-link", &second_link).expect("create second symlink");

    let error = MustNotSerialize
        .write_to(&first_link)
        .expect_err("symlink loop must fail preflight");

    assert_eq!(error.error_code(), ErrorCode::PolicyViolation);
    assert!(error.to_string().contains("unsafe symlink loop"));
    assert_path_context(&error, &first_link);
}

#[cfg(unix)]
#[test]
fn hard_linked_destination_is_rejected_without_breaking_identity() {
    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");
    let peer = directory.path().join("localizations-peer.txt");
    std::fs::write(&path, b"original").expect("write original destination");
    std::fs::hard_link(&path, &peer).expect("create hard link");

    let error = MustNotSerialize
        .write_to(&path)
        .expect_err("hard-linked destination must fail preflight");

    assert_eq!(error.error_code(), ErrorCode::PolicyViolation);
    assert!(error.to_string().contains("hard-linked"));
    assert_path_context(&error, &path);
    assert_eq!(std::fs::read(&path).expect("read destination"), b"original");
    assert_eq!(
        std::fs::read(&peer).expect("read hard-link peer"),
        b"original"
    );
}

#[cfg(unix)]
#[test]
fn read_only_destination_is_rejected_before_serialization() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("create test directory");
    let path = directory.path().join("localizations.txt");
    std::fs::write(&path, b"original").expect("write original destination");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))
        .expect("make destination read-only");

    let error = MustNotSerialize
        .write_to(&path)
        .expect_err("read-only destination must fail preflight");

    assert_eq!(error.error_code(), ErrorCode::Io);
    assert!(error.to_string().contains("read-only"));
    assert_path_context(&error, &path);
    assert_eq!(std::fs::read(&path).expect("read destination"), b"original");
}

#[test]
fn codec_write_resource_to_file_preserves_structured_path_error() {
    let directory = tempdir().expect("create test directory");
    let output = directory.path().join("output.strings");
    std::fs::create_dir(&output).expect("create invalid directory destination");

    let error = Codec::write_resource_to_file(
        &empty_resource(),
        output.to_str().expect("test path must be UTF-8"),
    )
    .expect_err("directory destination must fail");

    assert!(matches!(&error, Error::WithPath { .. }));
    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert_path_context(&error, &output);
}

#[test]
fn convert_resources_to_format_preserves_structured_path_error() {
    let directory = tempdir().expect("create test directory");
    let output = directory.path().join("output.strings");
    std::fs::create_dir(&output).expect("create invalid directory destination");

    let error = convert_resources_to_format(
        vec![empty_resource()],
        output.to_str().expect("test path must be UTF-8"),
        FormatType::Strings(Some("en".to_string())),
    )
    .expect_err("directory destination must fail");

    assert!(matches!(&error, Error::WithPath { .. }));
    assert_eq!(error.error_code(), ErrorCode::InvalidResource);
    assert_path_context(&error, &output);
}
