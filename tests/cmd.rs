use ptyprocess::PtyProcess;
use std::process::Command;

#[test]
fn empty() {
    let err = PtyProcess::spawn(Command::new("")).unwrap_err();

    match err.to_string().as_ref() {
        // ubuntu
        "ENXIO: No such device or address" | 
        // fedora
        "ENOENT: No such file or directory" => {},
        err => panic!("Unexpected error message: {}", err), 
    }
}
