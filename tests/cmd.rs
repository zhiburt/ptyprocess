use ptyprocess::PtyProcess;
use std::{io, process::Command};

#[test]
fn empty() {
    let err = PtyProcess::spawn(Command::new("")).unwrap_err();
    let os_err = err.as_errno().unwrap() as i32;
    assert_eq!(
        io::ErrorKind::NotFound,
        io::Error::from_raw_os_error(os_err).kind()
    );
}
