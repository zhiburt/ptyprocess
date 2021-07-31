use ptyprocess::{ControlCode, PtyProcess, Signal, WaitStatus};
use std::{process::Command, thread, time::Duration};

#[cfg(feature = "async")]
use futures_lite::{future::block_on, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

#[cfg(feature = "sync")]
use std::io::{BufRead, BufReader, LineWriter, Read, Write};

#[cfg(feature = "sync")]
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
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^C (occasional test hangs)
    // Ctrl-C is etx(End of text). Thus send \x03.
    thread::sleep(Duration::from_millis(300));

    p_write_all(&mut proc, &[3]).unwrap(); // send ^C
    proc_flush(&mut proc).unwrap();

    assert_eq!(
        proc.wait().unwrap(),
        WaitStatus::Signaled(proc.pid(), Signal::SIGINT, false),
    );
}

#[test]
fn cat_eof() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    // this sleep solves an edge case of some cases when cat is somehow not "ready"
    // to take the ^D (occasional test hangs)
    thread::sleep(Duration::from_millis(300));

    p_write_all(&mut proc, &[4]).unwrap(); // send ^D
    proc_flush(&mut proc).unwrap();

    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn read_after_process_exit() {
    let mut command = Command::new("echo");
    command.arg("hello cat");
    let mut proc = PtyProcess::spawn(command).unwrap();

    assert_eq!(p_read_to_end(&mut proc).unwrap(), b"hello cat\r\n");

    assert_eq!(0, p_read(&mut proc, &mut [0; 128]).unwrap());
    assert_eq!(0, p_read(&mut proc, &mut [0; 128]).unwrap());
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn ptyprocess_check_terminal_line_settings() {
    let mut command = Command::new("stty");
    command.arg("-a");
    let mut proc = PtyProcess::spawn(command).unwrap();

    let buf = p_read_to_string(&mut proc).unwrap();

    println!("{}", buf);

    assert!(buf.split_whitespace().any(|word| word == "-echo"));
}

#[test]
fn send_controll() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send_control(&mut proc, ControlCode::EOT).unwrap();

    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0),);
}

#[test]
fn send() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send(&mut proc, "hello cat\n").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello cat\r\n");

    assert!(proc.exit(true).unwrap());
}

#[test]
fn send_line() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send_line(&mut proc, "hello cat").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello cat\r\n");

    assert!(proc.exit(true).unwrap());
}

#[test]
fn try_read_byte() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(p_try_read_byte(&mut proc).unwrap(), None);

    p_send_line(&mut proc, "123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'1')));
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'2')));
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'3')));
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'\r')));
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'\n')));
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), None);
}

#[test]
fn blocking_read_after_non_blocking_try_read_byte() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(p_try_read_byte(&mut proc).unwrap(), None);

    p_send_line(&mut proc, "123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(p_try_read_byte(&mut proc).unwrap(), Some(Some(b'1')));

    let mut buf = [0; 64];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"23\r\n");

    thread::spawn(move || {
        let _ = p_read(&mut proc, &mut buf).unwrap();
        // the error will be propagated in case of panic
        panic!("it's unnexpected that read operation will be ended")
    });

    // give some time to read
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn try_read() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    let mut buf = vec![0; 128];
    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), None);

    p_send_line(&mut proc, "123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), Some(5));
    assert_eq!(&buf[..5], b"123\r\n");
    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), None);
}

#[test]
fn blocking_read_after_non_blocking_try_read() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    let mut buf = vec![0; 1];
    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), None);

    p_send_line(&mut proc, "123").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), Some(1));
    assert_eq!(&buf[..1], b"1");

    let mut buf = [0; 64];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"23\r\n");

    thread::spawn(move || {
        let _ = p_read(&mut proc, &mut buf).unwrap();
        // the error will be propagated in case of panic
        panic!("it's unnexpected that read operation will be ended")
    });

    // give some time to read
    thread::sleep(Duration::from_millis(100));
}

#[test]
fn try_read_after_eof() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send_line(&mut proc, "hello").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    let mut buf = vec![0; 128];
    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), Some(7));
    assert_eq!(p_try_read(&mut proc, &mut buf).unwrap(), None);
    assert_eq!(p_try_read_byte(&mut proc).unwrap(), None);
}

#[test]
fn try_read_after_process_exit() {
    let mut command = Command::new("echo");
    command.arg("hello cat");
    let mut proc = PtyProcess::spawn(command).unwrap();

    // on macos we may not able to read after process is dead.
    // I assume that kernel consumes proceses resorces without any code check of parent,
    // which what is happening on linux.
    //
    // So we check that there may be None or Some(0)

    let mut buf = vec![0; 128];
    assert!(matches!(
        p_try_read(&mut proc, &mut buf).unwrap(),
        Some(11) | None
    ));
    assert!(matches!(
        p_try_read(&mut proc, &mut buf).unwrap(),
        Some(0) | None
    ));

    // on macos we can't put it before read's for some reason something get blocked
    // assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));
}

#[test]
fn read_line() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send_line(&mut proc, "Hello World 1").unwrap();
    p_send_line(&mut proc, "Hello World 2").unwrap();

    assert_eq!(p_read_line(&mut proc).unwrap(), "Hello World 1\r\n");
    assert_eq!(p_read_line(&mut proc).unwrap(), "Hello World 2\r\n");
    assert!(proc.exit(true).unwrap());
}

#[test]
fn read_until() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    p_send_line(&mut proc, "Hello World 1").unwrap();

    // give cat a time to react on input
    thread::sleep(Duration::from_millis(100));

    assert_eq!(p_read_until(&mut proc, b' ').unwrap(), b"Hello ");

    let mut buf = vec![0; 128];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"World 1\r\n");

    assert!(proc.exit(true).unwrap());
}

#[test]
fn read_to_end() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let mut proc = PtyProcess::spawn(cmd).unwrap();

    assert_eq!(p_read_to_end(&mut proc).unwrap(), b"Hello World\r\n");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn read_to_end_after_delay() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let mut proc = PtyProcess::spawn(cmd).unwrap();

    thread::sleep(Duration::from_millis(500));

    assert_eq!(p_read_to_end(&mut proc).unwrap(), b"Hello World\r\n");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn read_after_process_is_gone() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let mut proc = PtyProcess::spawn(cmd).unwrap();

    // after we check a status of child
    // it should be marked DEAD.
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));

    // Just in case; make a little delay
    thread::sleep(Duration::from_millis(500));

    let mut buf = vec![0; 128];
    let n = p_read(&mut proc, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"Hello World\r\n");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn read_to_end_after_process_is_gone() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let mut proc = PtyProcess::spawn(cmd).unwrap();

    // after we check a status of child
    // it should be marked DEAD.
    assert_eq!(proc.wait().unwrap(), WaitStatus::Exited(proc.pid(), 0));

    // Just in case; make a little delay
    thread::sleep(Duration::from_millis(500));

    assert_eq!(p_read_to_end(&mut proc).unwrap(), b"Hello World\r\n");
}

#[test]
fn try_read_to_end() {
    let mut cmd = Command::new("echo");
    cmd.arg("Hello World");
    let mut proc = PtyProcess::spawn(cmd).unwrap();

    let mut buf = vec![0; 128];
    while p_try_read(&mut proc, &mut buf).unwrap() != Some(0) {}

    assert_eq!(&buf[..13], b"Hello World\r\n");
}

#[test]
fn continues_try_reads() {
    let mut cmd = Command::new("python3");
    cmd.args(vec![
        "-c",
        "import time;\
        print('Start Sleep');\
        time.sleep(0.1);\
        print('End of Sleep');\
        yn=input('input');",
    ]);

    let mut proc = PtyProcess::spawn(cmd).unwrap();

    let mut buf = [0; 128];
    loop {
        if let Some(n) = p_try_read(&mut proc, &mut buf).unwrap() {
            if String::from_utf8_lossy(&buf[..n]).contains("input") {
                break;
            }
        }
    }
}

#[test]
#[cfg(not(target_os = "macos"))]
fn automatic_stop_of_interact() {
    let mut p = PtyProcess::spawn(Command::new("ls")).unwrap();
    let status = p_interact(&mut p).unwrap();

    // It may be finished not only because process is done but
    // also because it reached EOF.
    assert!(matches!(
        status,
        WaitStatus::Exited(_, 0) | WaitStatus::StillAlive
    ));

    // check that second spawn works
    let mut p = PtyProcess::spawn(Command::new("ls")).unwrap();
    let status = p_interact(&mut p).unwrap();
    assert!(matches!(
        status,
        WaitStatus::Exited(_, 0) | WaitStatus::StillAlive
    ));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn spawn_after_interact() {
    let mut p = PtyProcess::spawn(Command::new("ls")).unwrap();
    let _ = p_interact(&mut p).unwrap();

    let p = PtyProcess::spawn(Command::new("ls")).unwrap();
    assert!(matches!(p.wait().unwrap(), WaitStatus::Exited(_, 0)));
}

fn p_read(proc: &mut PtyProcess, buf: &mut [u8]) -> std::io::Result<usize> {
    #[cfg(feature = "sync")]
    {
        proc.read(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.read(buf))
    }
}

fn p_write_all(proc: &mut PtyProcess, buf: &[u8]) -> std::io::Result<()> {
    #[cfg(feature = "sync")]
    {
        proc.write_all(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.write_all(buf))
    }
}

fn proc_flush(proc: &mut PtyProcess) -> std::io::Result<()> {
    #[cfg(feature = "sync")]
    {
        proc.flush()
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.flush())
    }
}

fn p_send(proc: &mut PtyProcess, buf: &str) -> std::io::Result<()> {
    #[cfg(feature = "sync")]
    {
        proc.send(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.send(buf))
    }
}

fn p_send_line(proc: &mut PtyProcess, buf: &str) -> std::io::Result<()> {
    #[cfg(feature = "sync")]
    {
        proc.send_line(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.send_line(buf))
    }
}

fn p_send_control(proc: &mut PtyProcess, buf: impl Into<ControlCode>) -> std::io::Result<()> {
    #[cfg(feature = "sync")]
    {
        proc.send_control(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.send_control(buf))
    }
}

fn p_read_to_string(proc: &mut PtyProcess) -> std::io::Result<String> {
    let mut buf = String::new();
    #[cfg(feature = "sync")]
    {
        proc.read_to_string(&mut buf)?;
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.read_to_string(&mut buf))?;
    }
    Ok(buf)
}

fn p_read_to_end(proc: &mut PtyProcess) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    #[cfg(feature = "sync")]
    {
        proc.read_to_end(&mut buf)?;
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.read_to_end(&mut buf))?;
    }
    Ok(buf)
}

fn p_read_until(proc: &mut PtyProcess, ch: u8) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    #[cfg(feature = "sync")]
    {
        let n = proc.read_until(ch, &mut buf)?;
        buf = buf[..n].to_vec();
    }
    #[cfg(feature = "async")]
    {
        let n = block_on(proc.read_until(ch, &mut buf))?;
        buf = buf[..n].to_vec();
    }
    Ok(buf)
}

fn p_read_line(proc: &mut PtyProcess) -> std::io::Result<String> {
    let mut buf = String::new();
    #[cfg(feature = "sync")]
    {
        proc.read_line(&mut buf)?;
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.read_line(&mut buf))?;
    }
    Ok(buf)
}

fn p_try_read_byte(proc: &mut PtyProcess) -> std::io::Result<Option<Option<u8>>> {
    #[cfg(feature = "sync")]
    {
        proc.try_read_byte()
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.try_read_byte())
    }
}

fn p_try_read(proc: &mut PtyProcess, buf: &mut [u8]) -> std::io::Result<Option<usize>> {
    #[cfg(feature = "sync")]
    {
        proc.try_read(buf)
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.try_read(buf))
    }
}

fn p_interact(proc: &mut PtyProcess) -> std::io::Result<WaitStatus> {
    #[cfg(feature = "sync")]
    {
        proc.interact()
    }
    #[cfg(feature = "async")]
    {
        block_on(proc.interact())
    }
}
