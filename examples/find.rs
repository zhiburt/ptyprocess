/// To run an example run the following command
/// `cargo run --example find`.
///
/// The example is based on https://github.com/zhiburt/ptyprocess/issues/2
use ptyprocess::PtyProcess;
use std::process::Command;

#[cfg(feature = "sync")]
fn main() {
    use std::io::BufRead;

    let mut cmd = Command::new("find");
    cmd.args(vec!["/home/", "-name", "foo"]);
    cmd.stderr(std::process::Stdio::null());

    let mut process = PtyProcess::spawn(cmd).unwrap();

    let mut buf = String::new();
    loop {
        match process.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                println!("buffer: {}", &buf[0..buf.len() - 1]); // Drop \n.
                buf.clear();
            }
            Err(e) => {
                println!("err: {}", e);
                break;
            }
        }
    }
}

#[cfg(feature = "async")]
fn main() {
    use futures_lite::AsyncBufReadExt;

    let mut cmd = Command::new("find");
    cmd.args(vec!["/home/", "-name", "foo"]);
    cmd.stderr(std::process::Stdio::null());

    let mut process = PtyProcess::spawn(cmd).unwrap();

    let mut buf = String::new();
    loop {
        match futures_lite::future::block_on(process.read_line(&mut buf)) {
            Ok(0) => break,
            Ok(_) => {
                println!("buffer: {}", &buf[0..buf.len() - 1]); // Drop \n.
                buf.clear();
            }
            Err(e) => {
                println!("err: {}", e);
                break;
            }
        }
    }
}
