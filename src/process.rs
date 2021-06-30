use crate::control_code::ControlCode;
use crate::stream::Stream;
use nix::fcntl::{open, FcntlArg, FdFlag, OFlag};
use nix::libc::{self, winsize, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use nix::pty::PtyMaster;
use nix::pty::{grantpt, posix_openpt, unlockpt};
use nix::sys::stat::Mode;
use nix::sys::wait::{self, waitpid, WaitStatus};
use nix::sys::{signal, termios};
use nix::unistd::{close, dup, dup2, fork, isatty, pipe, setsid, sysconf, write, ForkResult, Pid};
use nix::{ioctl_write_ptr_bad, Result};
use std::fs::File;
use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::os::unix::prelude::{AsRawFd, CommandExt, FromRawFd, RawFd};
use std::process::{self, Command};
use std::time::{self, Duration};
use std::{io, thread};
use termios::SpecialCharacterIndices;

pub const DEFAULT_TERM_COLS: u16 = 80;
pub const DEFAULT_TERM_ROWS: u16 = 24;
pub const DEFAULT_VEOF_CHAR: u8 = 0x4; // ^D
pub const DEFAULT_INTR_CHAR: u8 = 0x3; // ^C

#[derive(Debug)]
pub struct PtyProcess {
    master: Master,
    child_pid: Pid,
    stream: Stream,
    timeout: Option<Duration>,
    eof_char: u8,
    intr_char: u8,
}

impl PtyProcess {
    // make this result io::Result
    pub fn spawn(mut command: Command) -> Result<Self> {
        let eof_char = get_eof_char();
        let intr_char = get_intr_char();

        let master = Master::open()?;
        master.grant_slave_access()?;
        master.unlock_slave()?;

        // handle errors in child executions by pipe
        let (exec_err_pipe_read, exec_err_pipe_write) = pipe()?;

        let fork = unsafe { fork()? };
        match fork {
            ForkResult::Child => {
                let err = || -> nix::Result<()> {
                    let device = master.get_slave_name()?;
                    let slave_fd = master.get_slave_fd()?;
                    drop(master);

                    make_controlling_tty(&device)?;

                    redirect_std_streams(slave_fd)?;

                    set_echo(STDIN_FILENO, false)?;
                    set_term_size(STDIN_FILENO, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                    close(exec_err_pipe_read)?;
                    // close pipe on sucessfull exec
                    nix::fcntl::fcntl(exec_err_pipe_write, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;

                    //  Do not allow child to inherit open file descriptors from parent
                    //
                    // on linux could be used getrlimit(RLIMIT_NOFILE, rlim) interface
                    let max_open_fds = sysconf(nix::unistd::SysconfVar::OPEN_MAX)?.unwrap() as i32;
                    // Why closing FD 1 causes an endless loop
                    (3..max_open_fds)
                        .filter(|&fd| fd != slave_fd && fd != exec_err_pipe_write)
                        .for_each(|fd| {
                            let _ = close(fd);
                        });

                    let _ = command.exec();
                    Err(nix::Error::last())
                }()
                .unwrap_err();

                let code = err.as_errno().map_or(-1, |e| e as i32);

                write(exec_err_pipe_write, &code.to_be_bytes())?;

                process::exit(code);
            }
            ForkResult::Parent { child } => {
                close(exec_err_pipe_write)?;

                let mut pipe_buf = [0u8; 4];
                nix::unistd::read(exec_err_pipe_read, &mut pipe_buf)?;
                let code = i32::from_be_bytes(pipe_buf);
                if code != 0 {
                    return Err(nix::Error::from_errno(nix::errno::from_i32(code)));
                }

                set_term_size(master.as_raw_fd(), DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                let file = master.get_file_handle()?;
                let stream = Stream::new(file);

                Ok(Self {
                    master,
                    stream,
                    child_pid: child,
                    eof_char,
                    intr_char,
                    timeout: Some(Duration::from_millis(30000)),
                })
            }
        }
    }

    pub fn send(&mut self, s: &str) -> io::Result<()> {
        self.stream.write_all(s.as_bytes())
    }

    pub fn send_line(&mut self, s: &str) -> io::Result<()> {
        writeln!(self.stream, "{}", s)
    }

    pub fn send_control(&mut self, code: ControlCode) -> io::Result<()> {
        self.stream.write_all(&[code.into()])
    }

    pub fn send_eof(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.eof_char])
    }

    pub fn send_intr(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.intr_char])
    }

    pub fn pid(&self) -> Pid {
        self.child_pid
    }

    pub fn get_pty_handle(&self) -> Result<File> {
        self.master.get_file_handle()
    }

    pub fn get_window_size(&self) -> Result<(u16, u16)> {
        get_term_size(self.master.as_raw_fd())
    }

    pub fn set_window_size(&mut self, cols: u16, rows: u16) -> Result<()> {
        set_term_size(self.master.as_raw_fd(), cols, rows)
    }

    pub fn wait_echo(&self, on: bool, timeout: Option<Duration>) -> nix::Result<bool> {
        let now = time::Instant::now();
        while timeout.is_none() || now.elapsed() < timeout.unwrap() {
            if on == self.get_echo()? {
                return Ok(true);
            }

            thread::sleep(Duration::from_millis(100));
        }

        Ok(false)
    }

    pub fn get_echo(&self) -> nix::Result<bool> {
        termios::tcgetattr(self.master.as_raw_fd())
            .map(|flags| flags.local_flags.contains(termios::LocalFlags::ECHO))
    }

    pub fn set_echo(&mut self, on: bool) -> nix::Result<()> {
        set_echo(self.master.as_raw_fd(), on)
    }

    pub fn isatty(&self) -> Result<bool> {
        isatty(self.master.as_raw_fd())
    }

    pub fn set_exit_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }

    pub fn status(&self) -> Result<WaitStatus> {
        waitpid(self.child_pid, Some(wait::WaitPidFlag::WNOHANG))
    }

    pub fn kill(&mut self, signal: signal::Signal) -> Result<()> {
        signal::kill(self.child_pid, signal)
    }

    pub fn wait(&self) -> Result<WaitStatus> {
        waitpid(self.child_pid, None)
    }

    pub fn exit(&mut self) -> Result<WaitStatus> {
        if let Err(nix::Error::Sys(nix::errno::Errno::ESRCH)) = self.terminate(signal::SIGTERM) {
            return Ok(WaitStatus::Exited(self.child_pid, 0));
        }

        match self.status() {
            Err(nix::Error::Sys(nix::errno::Errno::ECHILD)) => {
                Ok(WaitStatus::Exited(self.child_pid, 0))
            }
            result => result,
        }
    }

    fn terminate(&mut self, signal: signal::Signal) -> Result<()> {
        let start = time::Instant::now();
        loop {
            self.kill(signal)?;

            let status = self.status()?;
            if status != wait::WaitStatus::StillAlive {
                return Ok(());
            }

            thread::sleep(time::Duration::from_millis(100));

            // kill -9 if timout is reached
            if let Some(timeout) = self.timeout {
                if start.elapsed() > timeout {
                    self.kill(signal::Signal::SIGKILL)?;
                    return Ok(());
                }
            }
        }
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        if let Ok(WaitStatus::StillAlive) = self.status() {
            self.exit().unwrap();
        }
    }
}

impl Deref for PtyProcess {
    type Target = Stream;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for PtyProcess {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

fn set_term_size(fd: i32, cols: u16, rows: u16) -> nix::Result<()> {
    ioctl_write_ptr_bad!(_set_window_size, libc::TIOCSWINSZ, winsize);

    let size = winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let _ = unsafe { _set_window_size(fd, &size) }?;

    Ok(())
}

fn get_term_size(fd: i32) -> nix::Result<(u16, u16)> {
    nix::ioctl_read_bad!(_get_window_size, libc::TIOCGWINSZ, winsize);

    let mut size = winsize {
        ws_col: 0,
        ws_row: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let _ = unsafe { _get_window_size(fd, &mut size) }?;

    Ok((size.ws_col, size.ws_row))
}

#[derive(Debug)]
pub(crate) struct Master {
    fd: PtyMaster,
}

impl Master {
    pub fn open() -> Result<Self> {
        let master_fd = posix_openpt(OFlag::O_RDWR)?;
        Ok(Self { fd: master_fd })
    }

    pub fn grant_slave_access(&self) -> Result<()> {
        grantpt(&self.fd)
    }

    pub fn unlock_slave(&self) -> Result<()> {
        unlockpt(&self.fd)
    }

    pub fn get_slave_name(&self) -> Result<String> {
        get_slave_name(&self.fd)
    }

    pub fn get_slave_fd(&self) -> Result<RawFd> {
        let slave_name = self.get_slave_name()?;
        let slave_fd = open(slave_name.as_str(), OFlag::O_RDWR, Mode::empty())?;
        Ok(slave_fd)
    }

    pub fn get_file_handle(&self) -> Result<File> {
        let fd = dup(self.as_raw_fd())?;
        let file = unsafe { File::from_raw_fd(fd) };

        Ok(file)
    }
}

impl AsRawFd for Master {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

#[cfg(not(target_os = "macos"))]
fn get_slave_name(fd: &PtyMaster) -> Result<String> {
    nix::pty::ptsname_r(fd)
}

/// Getting a slave name on darvin platform
/// https://blog.tarq.io/ptsname-on-osx-with-rust/
#[cfg(target_os = "macos")]
fn get_slave_name(fd: &PtyMaster) -> Result<String> {
    use nix::libc::ioctl;
    use nix::libc::TIOCPTYGNAME;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::os::unix::prelude::AsRawFd;

    // ptsname_r is a linux extension but ptsname isn't thread-safe
    // we could use a static mutex but instead we re-implemented ptsname_r with a syscall
    // ioctl(fd, TIOCPTYGNAME, buf) manually
    // the buffer size on OSX is 128, defined by sys/ttycom.h
    let mut buf: [c_char; 128] = [0; 128];

    let fd = fd.as_raw_fd();

    match unsafe { ioctl(fd, TIOCPTYGNAME as u64, &mut buf) } {
        0 => {
            let string = CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
            return Ok(string);
        }
        _ => Err(nix::Error::last()),
    }
}

fn redirect_std_streams(fd: RawFd) -> Result<()> {
    // If fildes2 is already a valid open file descriptor, it shall be closed first
    // but with this options it doesn't work properly...

    close(STDIN_FILENO)?;
    close(STDOUT_FILENO)?;
    close(STDERR_FILENO)?;

    // use slave fd as std[in/out/err]
    dup2(fd, STDIN_FILENO)?;
    dup2(fd, STDOUT_FILENO)?;
    dup2(fd, STDERR_FILENO)?;

    Ok(())
}

fn set_echo(fd: RawFd, on: bool) -> Result<()> {
    // Set echo off
    // Even though there may be something left behind https://stackoverflow.com/a/59034084
    let mut flags = termios::tcgetattr(fd)?;
    match on {
        true => flags.local_flags |= termios::LocalFlags::ECHO,
        false => flags.local_flags &= !termios::LocalFlags::ECHO,
    }

    termios::tcsetattr(STDIN_FILENO, termios::SetArg::TCSANOW, &flags)?;
    Ok(())
}

fn get_this_term_char(char: SpecialCharacterIndices) -> Option<u8> {
    for &fd in &[STDIN_FILENO, STDOUT_FILENO] {
        if let Ok(char) = get_term_char(fd, char) {
            return Some(char);
        }
    }

    None
}

fn get_intr_char() -> u8 {
    get_this_term_char(SpecialCharacterIndices::VINTR).unwrap_or(DEFAULT_INTR_CHAR)
}

fn get_eof_char() -> u8 {
    get_this_term_char(SpecialCharacterIndices::VEOF).unwrap_or(DEFAULT_VEOF_CHAR)
}

fn get_term_char(fd: RawFd, char: SpecialCharacterIndices) -> Result<u8> {
    let flags = termios::tcgetattr(fd)?;
    let b = flags.control_chars[char as usize];
    Ok(b)
}

fn make_controlling_tty(child_name: &str) -> Result<()> {
    // Is this appoach's result the same as just call ioctl TIOCSCTTY?

    // Disconnect from controlling tty, if any
    let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty())?;
    close(fd)?;

    setsid()?;

    // Verify we are disconnected from controlling tty by attempting to open
    // it again.  We expect that OSError of ENXIO should always be raised.
    let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty());
    match fd {
        Err(nix::Error::Sys(nix::errno::Errno::ENXIO)) => {} // ok
        Ok(fd) => {
            close(fd)?;
            return Err(nix::Error::UnsupportedOperation);
        }
        Err(_) => return Err(nix::Error::UnsupportedOperation),
    }

    // Verify we can open child pty.
    let fd = open(child_name, OFlag::O_RDWR, Mode::empty())?;
    close(fd)?;

    // Verify we now have a controlling tty.
    let fd = open("/dev/tty", OFlag::O_WRONLY, Mode::empty())?;
    close(fd)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_pty() -> Result<()> {
        let master = Master::open()?;
        master.grant_slave_access()?;
        master.unlock_slave()?;
        let slavename = master.get_slave_name()?;
        assert!(slavename.starts_with("/dev"));
        println!("slave name {}", slavename);
        Ok(())
    }

    #[test]
    #[ignore = "The test should be run in a sigle thread mode --jobs 1 or --test-threads 1"]
    fn release_pty_master() -> Result<()> {
        let master = Master::open()?;
        let old_master_fd = master.fd.as_raw_fd();

        drop(master);

        let master = Master::open()?;

        assert!(master.fd.as_raw_fd() == old_master_fd);

        Ok(())
    }
}
