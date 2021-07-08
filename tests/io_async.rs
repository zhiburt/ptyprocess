#![cfg(feature = "async")]

use futures_lite::{future::block_on, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
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
    block_on(async {
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
    block_on(async {
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
fn read_after_process_exit() {
    let msg = "hello cat";

    let mut command = Command::new("echo");
    command.arg(msg);
    let mut proc = PtyProcess::spawn(command).unwrap();

    thread::sleep(Duration::from_millis(300));

    block_on(async {
        let mut buf = Vec::new();
        proc.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, format!("{}\r\n", msg).as_bytes());

        assert_eq!(0, proc.read(&mut buf).await.unwrap());
        assert_eq!(0, proc.read(&mut buf).await.unwrap());

        // on macos this instruction must be at the as after parent checks child it's gone?
        assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
    })
}

#[test]
fn ptyprocess_check_terminal_line_settings() {
    let mut command = Command::new("stty");
    command.arg("-a");
    let mut proc = PtyProcess::spawn(command).unwrap();

    let mut buf = String::new();
    block_on(async {
        proc.read_to_string(&mut buf).await.unwrap();
    });
    println!("{}", buf);

    assert!(buf.split_whitespace().any(|word| word == "-echo"));
}

#[test]
fn send_controll() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
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

    block_on(async {
        process.send("hello cat\n").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(50));

        let mut buf = vec![0; 128];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello cat\r\n");
    });

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn send_line() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        process.send_line("hello cat").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(50));

        let mut buf = vec![0; 128];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello cat\r\n");
    });

    assert_eq!(process.exit(true).unwrap(), true);
}

#[test]
fn try_read_byte() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        assert_eq!(process.try_read_byte().await.unwrap(), None);

        process.send_line("123").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'1')));
        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'2')));
        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'3')));
        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'\r')));
        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'\n')));
        assert_eq!(process.try_read_byte().await.unwrap(), None);
    })
}

#[test]
fn blocking_read_after_non_blocking_try_read_byte() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        assert_eq!(process.try_read_byte().await.unwrap(), None);

        process.send_line("123").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        assert_eq!(process.try_read_byte().await.unwrap(), Some(Some(b'1')));

        let mut buf = [0; 64];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"23\r\n");

        thread::spawn(move || {
            // It must be allowed block_on
            let _ = block_on(process.read(&mut buf)).unwrap();
            // the error will be propagated in case of panic
            panic!("it's unnexpected that read operation will be ended")
        });

        // give some time to read
        thread::sleep(Duration::from_millis(100));
    });
}

#[test]
fn try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        let mut buf = vec![0; 128];
        assert_eq!(process.try_read(&mut buf).await.unwrap(), None);

        process.send_line("123").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        assert_eq!(process.try_read(&mut buf).await.unwrap(), Some(5));
        assert_eq!(&buf[..5], b"123\r\n");
        assert_eq!(process.try_read(&mut buf).await.unwrap(), None);
    });
}

#[test]
fn blocking_read_after_non_blocking_try_read() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        let mut buf = vec![0; 1];
        assert_eq!(process.try_read(&mut buf).await.unwrap(), None);

        process.send_line("123").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        assert_eq!(process.try_read(&mut buf).await.unwrap(), Some(1));
        assert_eq!(&buf[..1], b"1");

        let mut buf = [0; 64];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"23\r\n");

        thread::spawn(move || {
            let _ = block_on(process.read(&mut buf)).unwrap();
            // the error will be propagated in case of panic
            panic!("it's unnexpected that read operation will be ended")
        });

        // give some time to read
        thread::sleep(Duration::from_millis(100));
    });
}

#[test]
fn try_read_after_eof() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        process.send_line("hello").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        let mut buf = vec![0; 128];
        assert_eq!(process.try_read(&mut buf).await.unwrap(), Some(7));
        assert_eq!(process.try_read(&mut buf).await.unwrap(), None);
        assert_eq!(process.try_read_byte().await.unwrap(), None);
    });
}

#[test]
fn try_read_after_process_exit() {
    let mut command = Command::new("echo");
    command.arg("hello cat");
    let mut proc = PtyProcess::spawn(command).unwrap();

    thread::sleep(Duration::from_millis(300));

    block_on(async {
        let mut buf = vec![0; 128];
        assert_eq!(proc.try_read(&mut buf).await.unwrap(), Some(11));
        assert_eq!(&buf[..11], b"hello cat\r\n");
        
        // on macos next try read need some time
        thread::sleep(Duration::from_millis(300));

        assert_eq!(proc.try_read(&mut buf).await.unwrap(), Some(0));

        // on macos this instruction must be at the as after parent checks child it's gone?
        assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
    });
}

#[test]
fn read_line() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        process.send_line("Hello World 1").await.unwrap();
        process.send_line("Hello World 2").await.unwrap();

        let mut buf = String::new();
        process.read_line(&mut buf).await.unwrap();
        assert_eq!(&buf, "Hello World 1\r\n");

        let mut buf = String::new();
        process.read_line(&mut buf).await.unwrap();
        assert_eq!(&buf, "Hello World 2\r\n");

        assert_eq!(process.exit(true).unwrap(), true);
    })
}

#[test]
fn read_until() {
    let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();

    block_on(async {
        process.send_line("Hello World 1").await.unwrap();

        // give cat a time to react on input
        thread::sleep(Duration::from_millis(100));

        let mut buf = Vec::new();
        let n = process.read_until(b' ', &mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"Hello ");

        let mut buf = vec![0; 128];
        let n = process.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"World 1\r\n");

        assert_eq!(process.exit(true).unwrap(), true);
    })
}
