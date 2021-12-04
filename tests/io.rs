use ptyprocess::{PtyProcess, Signal, WaitStatus};
use std::{
    io::{BufRead, BufReader, LineWriter, Read, Write},
    process::Command,
    thread,
    time::Duration,
};

#[test]
fn custom_reader_writer() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let pty = proc.get_pty_handle().unwrap();
    let mut writer = LineWriter::new(&pty);
    let mut reader = BufReader::new(&pty);

    writer.write_all(b"hello cat\n").unwrap();
    let mut buf = String::new();
    reader.read_line(&mut buf).unwrap();
    assert_eq!(buf, "hello cat\r\n");

    drop(writer);
    drop(reader);

    assert!(proc.exit(true).unwrap());
}

#[test]
fn cat_intr() {
    let proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let mut w = proc.get_pty_handle().unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^C (occasional test hangs)
    // Ctrl-C is etx(End of text). Thus send \x03.
    thread::sleep(Duration::from_millis(300));

    w.write_all(&[3]).unwrap(); // send ^C
    w.flush().unwrap();

    assert_eq!(
        proc.wait().unwrap(),
        WaitStatus::Signaled(proc.pid(), Signal::SIGINT, false),
    );
}

#[test]
fn cat_eof() {
    let proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let mut w = proc.get_pty_handle().unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^D (occasional test hangs)
    thread::sleep(Duration::from_millis(300));

    w.write_all(&[4]).unwrap(); // send ^D
    w.flush().unwrap();

    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn read_more_then_process_gives() {
    let mut command = Command::new("echo");
    command.arg("hello cat");
    let proc = PtyProcess::spawn(command).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    let mut buf = Vec::new();
    w.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"hello cat\r\n");

    assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn read_after_process_exit() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    writeln!(w, "Hello").unwrap();

    let exited = proc.exit(true).unwrap();
    assert!(exited);

    assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    assert_eq!(0, w.read(&mut [0; 128]).unwrap());

    // on macos we can't write after proces is exited
    // on linux its ok
    if let Ok(_) = writeln!(w, "World") {
        assert_eq!(0, w.read(&mut [0; 128]).unwrap());
        assert_eq!(0, w.read(&mut [0; 128]).unwrap());
        assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    }
}

#[test]
fn ptyprocess_check_terminal_line_settings() {
    let mut command = Command::new("stty");
    command.arg("-a");
    let proc = PtyProcess::spawn(command).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    let mut buf = String::new();
    w.read_to_string(&mut buf).unwrap();

    println!("{}", buf);

    assert!(buf.split_whitespace().any(|word| word == "-echo"));
}

#[test]
fn read_line() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let w = proc.get_pty_handle().unwrap();
    let mut r = BufReader::new(&w);

    writeln!(&w, "Hello World 1").unwrap();
    writeln!(&w, "Hello World 2").unwrap();

    let mut buf = String::new();
    r.read_line(&mut buf).unwrap();
    assert_eq!(buf, "Hello World 1\r\n");

    let mut buf = String::new();
    r.read_line(&mut buf).unwrap();
    assert_eq!(buf, "Hello World 2\r\n");

    assert!(proc.exit(true).unwrap());
}

#[test]
fn read_until() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    let w = proc.get_pty_handle().unwrap();
    let mut r = BufReader::new(&w);

    writeln!(&w, "Hello World 1").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = Vec::new();
    r.read_until(b' ', &mut buf).unwrap();
    assert_eq!(buf, b"Hello ");

    let mut buf = vec![0; 128];
    let n = r.read(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"World 1\r\n");

    assert!(proc.exit(true).unwrap());
}

#[test]
fn read_to_end() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let proc = PtyProcess::spawn(cmd).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    // without a sleep we can't guarantee what we actually test
    std::thread::sleep(Duration::from_millis(500));

    let mut buf = Vec::new();
    w.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"Hello World\r\n");
}

#[test]
fn read_to_end_on_handle() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let proc = PtyProcess::spawn(cmd).unwrap();
    let mut w = proc.get_pty_handle().unwrap();

    #[cfg(target_os = "linux")]
    {
        let err = w.read_to_end(&mut Vec::new()).unwrap_err();
        assert_eq!(Some(5), err.raw_os_error());
    }

    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        let mut buf = Vec::new();
        let n = w.read_to_end(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello World\r\n");
    }
}

#[test]
fn read_to_end_after_delay() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let proc = PtyProcess::spawn(cmd).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    thread::sleep(Duration::from_millis(500));

    let mut buf = Vec::new();
    let n = w.read_to_end(&mut buf).unwrap();
    assert_eq!(&buf[..n], b"Hello World\r\n");
}

#[test]
fn read_after_process_is_gone() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let proc = PtyProcess::spawn(cmd).unwrap();
    let mut w = proc.get_pty_handle().unwrap();

    // after we check a status of child
    // it should be marked DEAD.
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));

    // Just in case; make a little delay
    thread::sleep(Duration::from_millis(500));

    #[cfg(target_os = "linux")]
    {
        let mut buf = vec![0; 128];
        let n = w.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"Hello World\r\n");
    }

    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        assert_eq!(0, w.read(&mut [0; 128]).unwrap());
    }
}

#[test]
fn read_to_end_after_process_is_gone() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let proc = PtyProcess::spawn(cmd).unwrap();
    let mut w = proc.get_pty_stream().unwrap();

    // after we check a status of child
    // it should be marked DEAD.
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));

    // Just in case; make a little delay
    thread::sleep(Duration::from_millis(500));

    #[cfg(target_os = "linux")]
    {
        let mut buf = Vec::new();
        w.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"Hello World\r\n")
    }

    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        let mut buf = Vec::new();
        w.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"")
    }
}
