/// To run an example run the following command
/// `cargo run --example find`.
///
/// The example is based on https://github.com/zhiburt/ptyprocess/issues/2
use ptyprocess::PtyProcess;
use std::io::{BufRead, BufReader};
use std::process::Command;

fn main() {
    let mut cmd = Command::new("find");
    cmd.args(vec!["/home/", "-name", "foo"]);
    cmd.stderr(std::process::Stdio::null());

    let process = PtyProcess::spawn(cmd).unwrap();
    let mut reader = BufReader::new(process.get_pty_handle().unwrap());

    let mut buf = String::new();
    loop {
        match reader.read_line(&mut buf) {
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
