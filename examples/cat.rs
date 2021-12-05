use ptyprocess::PtyProcess;
/// To run an example run the following command
/// `cargo run --example cat`.
use std::{
    fs::File,
    io::{self, Read, Write},
    process::Command,
};

fn main() {
    let process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");
    let mut stream = process
        .get_pty_stream()
        .expect("Failed to get a pty handle");

    let mut this_file = File::open(".gitignore").expect("Can't open a file");
    io::copy(&mut this_file, &mut stream).expect("Can't copy a file");

    // EOT
    stream
        .write_all(&[4])
        .expect("Error while exiting a process");

    // We can't read_to_end as the process isn't DEAD but at time time it is it's already a EOF

    let mut buf = [0; 128];
    loop {
        let n = stream.read(&mut buf).expect("Erorr on read");
        print!("{}", String::from_utf8_lossy(&buf[..n]));

        if n == 0 {
            break;
        }
    }
}
