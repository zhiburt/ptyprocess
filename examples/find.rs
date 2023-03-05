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
    let mut reader = BufReader::new(process.get_raw_handle().unwrap());

    let mut buf = String::new();
    loop {
        let n = reader.read_line(&mut buf).expect("readline error");
        if n == 0 {
            break;
        }

        // by -1 we drop \n.
        let text = &buf[0..buf.len() - 1];
        println!("buffer: {text}");

        buf.clear();
    }
}
