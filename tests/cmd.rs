use ptyprocess::PtyProcess;
use std::{io, process::Command};

#[test]
fn empty() {
    let err = PtyProcess::spawn(Command::new("")).unwrap_err();
    assert_eq!(
        io::ErrorKind::NotFound,
        io::Error::from_raw_os_error(err as i32).kind()
    );
}
