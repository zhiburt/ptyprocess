/// To run an example run the following command
/// `cargo run --example interact`.
use ptyprocess::PtyProcess;
use std::process::Command;

#[cfg(feature = "sync")]
fn main() {
    let mut p = PtyProcess::spawn(Command::new("cat")).unwrap();

    println!("Now you're in interacting mode");
    println!("To return control back to main type CTRL-]");

    let status = p.interact().expect("Failed to start interact");

    println!("Quiting status {:?}", status);
}

#[cfg(feature = "async")]
fn main() {
    let mut p = PtyProcess::spawn(Command::new("cat")).unwrap();

    println!("Now you're in interacting mode");
    println!("To return control back to main type CTRL-]");

    let status = futures_lite::future::block_on(p.interact()).expect("Failed to start interact");

    println!("Quiting status {:?}", status);
}
