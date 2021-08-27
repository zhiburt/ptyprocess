/// To run an example run the following command
/// `cargo run --example cat`.

use std::{
    fs::File,
    io::{self, Read, Write},
    process::Command,
};
use ptyprocess::PtyProcess;

fn main() {
    let process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");
    let mut proc_io = process
        .get_pty_stream()
        .expect("Failed to get a pty handle");

    let mut this_file = File::open(".gitignore").expect("Can't open a file");

    io::copy(&mut this_file, &mut proc_io).expect("Can't copy a file");

    // EOT
    proc_io
        .write_all(&[4])
        .expect("Error while exiting a process");

    // We can't read_to_end as the process isn't DEAD but at time time it is it's already a EOF
    let mut file = Vec::new();
    proc_io.read_to_end(&mut file).expect("Erorr on read");

    println!("{}", String::from_utf8_lossy(&file));
}
