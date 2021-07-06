#![cfg(feature = "async")]

use futures_lite::{future, AsyncReadExt, AsyncWriteExt};
use ptyprocess::{ControlCode, PtyProcess, Signal, WaitStatus};
use std::{
    io::{BufRead, BufReader, LineWriter, Write},
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
    future::block_on(async {
        let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

        // this sleep solves an edge case of some cases when cat is somehow not "ready"
        // to take the ^C (occasional test hangs)
        // Ctrl-C is etx(End of text). Thus send \x03.
        thread::sleep(Duration::from_millis(300));
        process.write_all(&[3]).await.unwrap(); // send ^C
        process.flush().await.unwrap();

        let status = process.wait().unwrap();

        assert_eq!(
            WaitStatus::Signaled(process.pid(), Signal::SIGINT, false),
            status
        );
    })
}

#[test]
fn cat_eof() {
    future::block_on(async {
        let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

        // this sleep solves an edge case of some cases when cat is somehow not "ready"
        // to take the ^D (occasional test hangs)
        thread::sleep(Duration::from_millis(300));
        proc.write_all(&[4]).await.unwrap(); // send ^D
        proc.flush().await.unwrap();

        let status = proc.wait().unwrap();

        assert_eq!(WaitStatus::Exited(proc.pid(), 0), status);
    })
}

#[test]
fn read_after_eof() {
    let msg = "hello cat";

    let mut command = Command::new("echo");
    command.arg(msg);
    let mut proc = PtyProcess::spawn(command).unwrap();

    future::block_on(async {
        let mut buf = Vec::new();
        proc.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, format!("{}\r\n", msg).as_bytes());

        assert_eq!(0, proc.read(&mut buf).await.unwrap());
        assert_eq!(0, proc.read(&mut buf).await.unwrap());

        assert_eq!(WaitStatus::Exited(proc.pid(), 0), proc.wait().unwrap());
    })
}

#[test]
fn ptyprocess_check_terminal_line_settings() {
    let mut command = Command::new("stty");
    command.arg("-a");
    let mut proc = PtyProcess::spawn(command).unwrap();

    let mut buf = String::new();
    future::block_on(async {
        proc.read_to_string(&mut buf).await.unwrap();
    });
    println!("{}", buf);

    assert!(buf.split_whitespace().any(|word| word == "-echo"));
}

#[test]
fn send_controll() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    future::block_on(async {
        process.send_control(ControlCode::EOT).await.unwrap();
    });

    assert_eq!(
        WaitStatus::Exited(process.pid(), 0),
        process.wait().unwrap()
    );
}

#[test]
fn send() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    future::block_on(async {
        process.send("hello cat\n").await.unwrap();
        let mut buf = vec![0; 128];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello cat\r\n");
    });

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn send_line() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    future::block_on(async {
        process.send_line("hello cat").await.unwrap();
        let mut buf = vec![0; 128];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello cat\r\n");
    });

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(process.try_read().unwrap(), None);

    future::block_on(process.send_line("123")).unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read().unwrap(), Some(b'1'));
    assert_eq!(process.try_read().unwrap(), Some(b'2'));
    assert_eq!(process.try_read().unwrap(), Some(b'3'));
    assert_eq!(process.try_read().unwrap(), Some(b'\r'));
    assert_eq!(process.try_read().unwrap(), Some(b'\n'));
    assert_eq!(process.try_read().unwrap(), None);
}

#[test]
fn blocking_read_after_non_blocking_try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(process.try_read().unwrap(), None);

    future::block_on(process.send_line("123")).unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(process.try_read().unwrap(), Some(b'1'));

    let mut buf = [0; 64];
    let n = future::block_on(process.read(&mut buf)).unwrap();
    assert_eq!(&buf[..n], b"23\r\n");

    thread::spawn(move || {
        let _ = future::block_on(process.read(&mut buf)).unwrap();
        // the error will be propagated in case of panic
        panic!("it's unnexpected that read operation will be ended")
    });

    // give some time to read
    thread::sleep(Duration::from_millis(100));
}
