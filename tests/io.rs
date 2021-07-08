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
fn read_after_process_exit() {
    let msg = "hello cat";

    let mut command = Command::new("echo");
    command.arg(msg);
    let mut proc = PtyProcess::spawn(command).unwrap();

    let mut buf = Vec::new();
    proc.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, format!("{}\r\n", msg).as_bytes());

    assert_eq!(0, proc.read(&mut buf).unwrap());
    assert_eq!(0, proc.read(&mut buf).unwrap());

    // on macos this instruction must be at the as after parent checks child it's gone?
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
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

#[test]
fn send() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send("hello cat\n").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    let n = process.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello cat\r\n");

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn send_line() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send_line("hello cat").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    let n = process.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello cat\r\n");

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn try_read_byte() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(process.try_read_byte().unwrap(), None);

    process.send_line("123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'1')));
    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'2')));
    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'3')));
    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'\r')));
    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'\n')));
    assert_eq!(process.try_read_byte().unwrap(), None);
}

#[test]
fn blocking_read_after_non_blocking_try_read_byte() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(process.try_read_byte().unwrap(), None);

    process.send_line("123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read_byte().unwrap(), Some(Some(b'1')));

    let mut buf = [0; 64];
    let n = process.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"23\r\n");

    thread::spawn(move || {
        let _ = process.read(&mut buf).unwrap();
        // the error will be propagated in case of panic
        panic!("it's unnexpected that read operation will be ended")
    });

    // give some time to read
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    let mut buf = vec![0; 128];
    assert_eq!(process.try_read(&mut buf).unwrap(), None);

    process.send_line("123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read(&mut buf).unwrap(), Some(5));
    assert_eq!(&buf[..5], b"123\r\n");
    assert_eq!(process.try_read(&mut buf).unwrap(), None);
}

#[test]
fn blocking_read_after_non_blocking_try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    let mut buf = vec![0; 1];
    assert_eq!(process.try_read(&mut buf).unwrap(), None);

    process.send_line("123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read(&mut buf).unwrap(), Some(1));
    assert_eq!(&buf[..1], b"1");

    let mut buf = [0; 64];
    let n = process.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"23\r\n");

    thread::spawn(move || {
        let _ = process.read(&mut buf).unwrap();
        // the error will be propagated in case of panic
        panic!("it's unnexpected that read operation will be ended")
    });

    // give some time to read
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn try_read_after_eof() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send_line("hello").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    assert_eq!(process.try_read(&mut buf).unwrap(), Some(7));
    assert_eq!(process.try_read(&mut buf).unwrap(), None);
    assert_eq!(process.try_read_byte().unwrap(), None);
}

#[test]
fn try_read_after_process_exit() {
    let msg = "hello cat";

    let mut command = Command::new("echo");
    command.arg(msg);
    let mut proc = PtyProcess::spawn(command).unwrap();

    // on macos we may not able to read after process is dead.
    // I assume that kernel consumes proceses resorces without any code check of parent,
    // which what is happening on linux.
    //
    // So we check that there may be None or Some(0)

    let mut buf = vec![0; 128];
    assert!(matches!(proc.try_read(&mut buf).unwrap(), Some(11) | None));
    assert!(matches!(proc.try_read(&mut buf).unwrap(), Some(0) | None));

    // on macos we can't put it before read's for some reason something get blocked
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn read_line() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send_line("Hello World 1").unwrap();
    process.send_line("Hello World 2").unwrap();

    let mut buf = String::new();
    process.read_line(&mut buf).unwrap();
    assert_eq!(&buf, "Hello World 1\r\n");

    let mut buf = String::new();
    process.read_line(&mut buf).unwrap();
    assert_eq!(&buf, "Hello World 2\r\n");

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn read_until() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    process.send_line("Hello World 1").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = Vec::new();
    let n = process.read_until(b' ', &mut buf).unwrap();
    assert_eq!(&buf[..n], b"Hello ");

    let mut buf = vec![0; 128];
    let n = process.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"World 1\r\n");

    assert_eq!(process.exit(true).unwrap(), true);
}
