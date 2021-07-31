/// To run an example run the following command
/// `cargo run --example cat`.

#[cfg(feature = "sync")]
fn main() {
    use ptyprocess::{ControlCode, PtyProcess};
    use std::{
        fs::File,
        io::{self, Read},
        ops::DerefMut,
        process::Command,
    };

    let mut process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");

    let mut this_file = File::open(".gitignore").expect("Can't open a file");

    io::copy(&mut this_file, process.deref_mut()).expect("Can't copy a file");

    process
        .send_control(ControlCode::EndOfTransmission)
        .expect("Error while exiting a process");

    // We can't read_to_end as the process isn't DEAD but at time time it is it's already a EOF
    let mut file = Vec::new();
    process.read_to_end(&mut file).expect("Erorr on read");

    println!("{}", String::from_utf8_lossy(&file));
}

#[cfg(feature = "async")]
fn main() {
    todo!("Use a sync version for this example");
}
