/// To run an example run the following command
/// `cargo run --example cat`.
use ptyprocess::PtyProcess;
use std::{fs::File, io, ops::DerefMut, process::Command};

#[cfg(feature = "sync")]
fn main() {
    use std::io::BufRead;

    let mut process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");

    let mut this_file = File::open(".gitignore").expect("Can't open a file");

    io::copy(&mut this_file, process.deref_mut()).expect("Can't copy a file");

    // We can't read_to_end as the process isn't DEAD but at time time it is it's already a EOF
    let mut file = Vec::new();
    let mut buf = String::new();
    // 10 - count lines in this file
    for _ in 0..10 {
        let n = process
            .read_line(&mut buf)
            .expect("Failed to read from a cat");
        file.extend_from_slice(&buf.as_bytes()[..n]);
    }

    process
        .send_control('C')
        .expect("Error while exiting a process");

    println!("{}", String::from_utf8_lossy(&file));
}

#[cfg(feature = "async")]
fn main() {
    todo!("Use a sync version for this example");
}
