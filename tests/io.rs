#![cfg(feature = "sync")]

use ptyprocess::{ControlCode, PtyProcess, Signal, WaitStatus};
use std::{
    io::{BufRead, BufReader, LineWriter, Read, Write},
    process::Command,
    thread,
    time::Duration,
};

#[test]
fn cat() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();
    let pty = process.get_pty_handle().unwrap();
    let mut writer = LineWriter::new(&pty);
    let mut reader = BufReader::new(&pty);

    writer.write_all(b"hello cat\n").unwrap();
    let mut buf = String::new();
    reader.read_line(&mut buf).unwrap();
    assert_eq!(buf, "hello cat\r\n");

    drop(writer);
    drop(reader);

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn cat_intr() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^C (occasional test hangs)
    // Ctrl-C is etx(End of text). Thus send \x03.
    thread::sleep(Duration::from_millis(300));
    process.write_all(&[3]).unwrap(); // send ^C
    process.flush().unwrap();

    let status = process.wait().unwrap();

    assert_eq!(
        WaitStatus::Signaled(process.pid(), Signal::SIGINT, false),
        status
    );
}

#[test]
fn cat_eof() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^D (occasional test hangs)
    thread::sleep(Duration::from_millis(300));
    proc.write_all(&[4]).unwrap(); // send ^D
    proc.flush().unwrap();

    let status = proc.wait().unwrap();

    assert_eq!(WaitStatus::Exited(proc.pid(), 0), status);
}

#[test]
fn read_after_eof() {
    let msg = "hello cat";

    let mut command = Command::new("echo");
    command.arg(msg);
    let mut proc = PtyProcess::spawn(command).unwrap();

    let mut buf = Vec::new();
    proc.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, format!("{}\r\n", msg).as_bytes());

    assert_eq!(0, proc.read(&mut buf).unwrap());
    assert_eq!(0, proc.read(&mut buf).unwrap());

    assert_eq!(WaitStatus::Exited(proc.pid(), 0), proc.wait().unwrap());
}

#[test]
fn ptyprocess_check_terminal_line_settings() {
    let mut command = Command::new("stty");
    command.arg("-a");
    let mut proc = PtyProcess::spawn(command).unwrap();

    let mut buf = String::new();
    proc.read_to_string(&mut buf).unwrap();

    println!("{}", buf);

    assert!(buf.split_whitespace().any(|word| word == "-echo"));
}

#[test]
fn send_controll() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send_control(ControlCode::EOT).unwrap();

    assert_eq!(
        WaitStatus::Exited(process.pid(), 0),
        process.wait().unwrap()
    );
}
