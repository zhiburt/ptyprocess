/// To run an example run the following command
/// `cargo run --example interact`.
use ptyprocess::PtyProcess;
use std::process::Command;

#[cfg(feature = "sync")]
fn main() {
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");

    println!("Now you're in interacting mode");
    println!("To return control back to main type CTRL-]");

    let status = process.interact().expect("Failed to start interact");

    println!("Status {:?}", status);
}

#[cfg(feature = "async")]
fn main() {
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("Error while spawning process");

    println!("Now you're in interacting mode");
    println!("To return control back to main type CTRL-]");

    futures_lite::future::block_on(process.interact()).expect("Failed to start interact");

    println!("Quiting");
}
