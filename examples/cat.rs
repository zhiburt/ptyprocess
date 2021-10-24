use ptyprocess::PtyProcess;
/// To run an example run the following command
/// `cargo run --example cat`.
use std::{
    fs::File,
    io::{self, Read, Write},
    process::Command,
};

fn main() {
    let mut cmd = Command::new("echo");
    cmd.arg("hello world");
    let mut process = PtyProcess::spawn(cmd).expect("Error while spawning process");
    println!("w8");
    process.wait();
    println!("done");
}
