use ptyprocess::PtyProcess;
/// To run an example run the following command
/// `cargo run --example cat`.
use std::{
    fs::File,
    io::{self, Read, Write},
    process::Command,
};

fn main() {
    let mut process = PtyProcess::spawn(Command::new("echo").arg("hello world")).expect("Error while spawning process");
    println!("w8");
    process.wait();
    println!("done");
}
