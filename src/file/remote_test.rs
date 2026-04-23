use std::path::PathBuf;

use super::*;

#[test]
fn remote_file_with_key_path_sets_field() {
    let remote = RemoteFile::new("host".into(), 22, "user".into(), "secret".into())
        .with_key_path("~/.ssh/id_rsa".into());

    assert_eq!(remote.host, "host");
    assert_eq!(remote.port, 22);
    assert_eq!(remote.user, "user");
    assert_eq!(remote.password, "secret");
    assert_eq!(remote.key_path.as_deref(), Some("~/.ssh/id_rsa"));
}

#[cfg(unix)]
#[tokio::test]
async fn remote_file_rejects_non_utf8_paths_before_network_access() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let remote = RemoteFile::new("127.0.0.1".into(), 22, "user".into(), "secret".into());
    let invalid = PathBuf::from(OsStr::from_bytes(b"/tmp/\xFFinvalid"));

    let read_err = remote.read(&invalid, None).await.unwrap_err().to_string();
    assert!(read_err.contains("non-utf8 remote path"));

    let write_err = remote
        .write(&invalid, "data", None)
        .await
        .unwrap_err()
        .to_string();
    assert!(write_err.contains("non-utf8 remote path"));
}
