use crate::control_code::ControlCode;
use crate::stream::Stream;
use nix::errno::{self, Errno};
use nix::fcntl::{fcntl, open, FcntlArg, FdFlag, OFlag};
use nix::libc::{self, winsize, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use nix::pty::PtyMaster;
use nix::pty::{grantpt, posix_openpt, unlockpt};
use nix::sys::stat::Mode;
use nix::sys::wait::{self, waitpid, WaitStatus};
use nix::sys::{signal, termios};
use nix::unistd::{
    self, close, dup, dup2, fork, isatty, pipe, setsid, sysconf, write, ForkResult, Pid, SysconfVar,
};
use nix::{ioctl_write_ptr_bad, Error, Result};
use signal::Signal::SIGKILL;
use std::fs::File;
use std::io::IoSlice;
use std::ops::{Deref, DerefMut};
use std::os::unix::prelude::{AsRawFd, CommandExt, FromRawFd, RawFd};
use std::process::{self, Command};
use std::time::{self, Duration};
use std::{io, thread};
use termios::SpecialCharacterIndices;

const DEFAULT_TERM_COLS: u16 = 80;
const DEFAULT_TERM_ROWS: u16 = 24;
const DEFAULT_VEOF_CHAR: u8 = 0x4; // ^D
const DEFAULT_INTR_CHAR: u8 = 0x3; // ^C

/// PtyProcess represents a controller for a spawned process.
///
/// The structure implements `std::io::Read` and `std::io::Write` which communicates with
/// a child.
///
/// ```no_run
/// use ptyprocess::PtyProcess;
/// use std::io::Write;
/// use std::process::Command;
///
/// let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();
/// process.write_all(b"Hello World").unwrap();
/// process.flush().unwrap();
/// ```
#[derive(Debug)]
pub struct PtyProcess {
    master: Master,
    child_pid: Pid,
    stream: Stream,
    eof_char: u8,
    intr_char: u8,
    terminate_approach_delay: Duration,
}

impl PtyProcess {
    /// Spawns a child process and
    /// creates a `PtyProcess` structure which controll communication with a child
    ///
    /// ```no_run
    ///   # use std::process::Command;
    ///   # use ptyprocess::PtyProcess;
    ///     let proc = PtyProcess::spawn(Command::new("bash"));
    /// ```
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
                let err = || -> Result<()> {
                    let device = master.get_slave_name()?;
                    let slave_fd = master.get_slave_fd()?;
                    drop(master);

                    make_controlling_tty(&device)?;
                    redirect_std_streams(slave_fd)?;

                    set_echo(STDIN_FILENO, false)?;
                    set_term_size(STDIN_FILENO, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                    close(exec_err_pipe_read)?;
                    // close pipe on sucessfull exec
                    fcntl(exec_err_pipe_write, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;

                    // Do not allow child to inherit open file descriptors from parent
                    //
                    // on linux could be used getrlimit(RLIMIT_NOFILE, rlim) interface
                    let max_open_fds = sysconf(SysconfVar::OPEN_MAX)?.unwrap() as i32;
                    // Why closing FD 1 causes an endless loop
                    (3..max_open_fds)
                        .filter(|&fd| fd != slave_fd && fd != exec_err_pipe_write)
                        .for_each(|fd| {
                            let _ = close(fd);
                        });

                    let _ = command.exec();
                    Err(Error::last())
                }()
                .unwrap_err();

                let code = err.as_errno().map_or(-1, |e| e as i32);

                write(exec_err_pipe_write, &code.to_be_bytes())?;

                process::exit(code);
            }
            ForkResult::Parent { child } => {
                close(exec_err_pipe_write)?;

                let mut pipe_buf = [0u8; 4];
                unistd::read(exec_err_pipe_read, &mut pipe_buf)?;
                let code = i32::from_be_bytes(pipe_buf);
                if code != 0 {
                    return Err(Error::from_errno(errno::from_i32(code)));
                }

                // Some systems may work in this way? (not sure)
                // that we need to set a terminal size in a parent.
                set_term_size(master.as_raw_fd(), DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                let file = master.get_file_handle()?;
                let stream = Stream::new(file);

                Ok(Self {
                    master,
                    stream,
                    child_pid: child,
                    eof_char,
                    intr_char,
                    terminate_approach_delay: Duration::from_millis(100),
                })
            }
        }
    }

    /// Returns a pid of a child process
    pub fn pid(&self) -> Pid {
        self.child_pid
    }

    /// Returns a file representation of a PTY, which can be used to communicate with it.
    ///
    /// ```no_run
    /// use ptyprocess::PtyProcess;
    /// use std::{process::Command, io::{BufReader, LineWriter}};
    ///
    /// let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();
    /// let pty = process.get_pty_handle().unwrap();
    /// let mut writer = LineWriter::new(&pty);
    /// let mut reader = BufReader::new(&pty);
    /// ```
    pub fn get_pty_handle(&self) -> Result<File> {
        self.master.get_file_handle()
    }

    /// Get window size of a terminal.
    ///
    /// Default is size is 80x24.
    pub fn get_window_size(&self) -> Result<(u16, u16)> {
        get_term_size(self.master.as_raw_fd())
    }

    /// Sets a terminal size
    pub fn set_window_size(&mut self, cols: u16, rows: u16) -> Result<()> {
        set_term_size(self.master.as_raw_fd(), cols, rows)
    }

    /// Waits until a echo settings were setup
    pub fn wait_echo(&self, on: bool, timeout: Option<Duration>) -> Result<bool> {
        let now = time::Instant::now();
        while timeout.is_none() || now.elapsed() < timeout.unwrap() {
            if on == self.get_echo()? {
                return Ok(true);
            }

            thread::sleep(Duration::from_millis(100));
        }

        Ok(false)
    }

    /// Get_echo returns true if an echo setting is setup.
    pub fn get_echo(&self) -> Result<bool> {
        termios::tcgetattr(self.master.as_raw_fd())
            .map(|flags| flags.local_flags.contains(termios::LocalFlags::ECHO))
    }

    /// Sets a echo setting for a terminal
    pub fn set_echo(&mut self, on: bool) -> Result<()> {
        set_echo(self.master.as_raw_fd(), on)
    }

    pub fn isatty(&self) -> Result<bool> {
        isatty(self.master.as_raw_fd())
    }

    /// Set the pty process's terminate approach delay.
    pub fn set_terminate_approach_delay(&mut self, terminate_approach_delay: Duration) {
        self.terminate_approach_delay = terminate_approach_delay;
    }

    /// Status returns a status a of child process.
    pub fn status(&self) -> Result<WaitStatus> {
        waitpid(self.child_pid, Some(wait::WaitPidFlag::WNOHANG))
    }

    /// Kill sends a signal to a child process.
    ///
    /// The operation is non-blocking.
    pub fn kill(&mut self, signal: signal::Signal) -> Result<()> {
        signal::kill(self.child_pid, signal)
    }

    /// Signal is an alias to kill.
    pub fn signal(&mut self, signal: signal::Signal) -> Result<()> {
        self.kill(signal)
    }

    /// Wait blocks until a child process exits.
    ///
    /// It returns a error if the child was DEAD or not exist
    /// at the time of a call.
    /// So sometimes it's better to use a [`is_alive`] method
    ///
    /// [`is_alive`]: struct.PtyProcess.html#method.is_alive
    pub fn wait(&self) -> Result<WaitStatus> {
        waitpid(self.child_pid, None)
    }

    /// Checks if a process is still exists.
    ///
    /// It's a non blocking operation.
    /// It's takes in mind errors which indicates that the child is gone.
    pub fn is_alive(&self) -> Result<bool> {
        match self.status() {
            Ok(status) if status == WaitStatus::StillAlive => Ok(true),
            Ok(_) | Err(Error::Sys(Errno::ECHILD)) | Err(Error::Sys(Errno::ESRCH)) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Try to force a child to terminate.
    ///
    /// It starts nicely with
    /// SIGHUP, SIGCONT, SIGINT, SIGTERM.
    /// If "force" is True then moves onto SIGKILL.
    ///
    /// This returns true if the child was terminated. and returns false if the
    /// child could not be terminated.
    pub fn exit(&mut self, force: bool) -> Result<bool> {
        if !self.is_alive()? {
            return Ok(true);
        }

        for &signal in &[
            signal::SIGHUP,
            signal::SIGCONT,
            signal::SIGINT,
            signal::SIGTERM,
        ] {
            if self.try_to_terminate(signal)? {
                return Ok(true);
            }
        }

        if !force {
            return Ok(false);
        }

        self.try_to_terminate(SIGKILL)
    }

    fn try_to_terminate(&mut self, signal: signal::Signal) -> Result<bool> {
        self.kill(signal)?;
        thread::sleep(self.terminate_approach_delay);

        self.is_alive().map(|is_alive| !is_alive)
    }
}

#[cfg(feature = "sync")]
use std::io::Write;

#[cfg(feature = "sync")]
impl PtyProcess {
    /// Send writes a string to a STDIN of a child.
    pub fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.stream.write_all(s.as_ref().as_bytes())
    }

    /// Send writes a line to a STDIN of a child.
    pub fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        #[cfg(windows)]
        const LINE_ENDING: &[u8] = b"\r\n";
        #[cfg(not(windows))]
        const LINE_ENDING: &[u8] = b"\n";

        let bufs = &mut [
            IoSlice::new(s.as_ref().as_bytes()),
            IoSlice::new(LINE_ENDING),
        ];

        let _ = self.write_vectored(bufs)?;
        self.flush()?;

        Ok(())
    }

    /// Send controll character to a child process.
    pub fn send_control(&mut self, code: ControlCode) -> io::Result<()> {
        self.stream.write_all(&[code.into()])
    }

    /// Send EOF indicator to a child process.
    pub fn send_eof(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.eof_char])
    }

    /// Send INTR indicator to a child process.
    pub fn send_intr(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.intr_char])
    }
}

#[cfg(feature = "async")]
use futures_lite::AsyncWriteExt;

#[cfg(feature = "async")]
impl PtyProcess {
    /// Send writes a string to a STDIN of a child.
    pub async fn send<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        self.stream.write_all(s.as_ref().as_bytes()).await
    }

    /// Send writes a line to a STDIN of a child.
    pub async fn send_line<S: AsRef<str>>(&mut self, s: S) -> io::Result<()> {
        #[cfg(windows)]
        const LINE_ENDING: &[u8] = b"\r\n";
        #[cfg(not(windows))]
        const LINE_ENDING: &[u8] = b"\n";

        let _ = self.write_all(s.as_ref().as_bytes()).await?;
        let _ = self.write_all(LINE_ENDING).await?;
        self.flush().await?;

        Ok(())
    }

    /// Send controll character to a child process.
    pub async fn send_control(&mut self, code: ControlCode) -> io::Result<()> {
        self.stream.write_all(&[code.into()]).await
    }

    /// Send EOF indicator to a child process.
    pub async fn send_eof(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.eof_char]).await
    }

    /// Send INTR indicator to a child process.
    pub async fn send_intr(&mut self) -> io::Result<()> {
        self.stream.write_all(&[self.intr_char]).await
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        if let Ok(WaitStatus::StillAlive) = self.status() {
            self.exit(true).unwrap();
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

fn set_term_size(fd: i32, cols: u16, rows: u16) -> Result<()> {
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

fn get_term_size(fd: i32) -> Result<(u16, u16)> {
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
struct Master {
    fd: PtyMaster,
}

impl Master {
    fn open() -> Result<Self> {
        let master_fd = posix_openpt(OFlag::O_RDWR)?;
        Ok(Self { fd: master_fd })
    }

    fn grant_slave_access(&self) -> Result<()> {
        grantpt(&self.fd)
    }

    fn unlock_slave(&self) -> Result<()> {
        unlockpt(&self.fd)
    }

    fn get_slave_name(&self) -> Result<String> {
        get_slave_name(&self.fd)
    }

    fn get_slave_fd(&self) -> Result<RawFd> {
        let slave_name = self.get_slave_name()?;
        let slave_fd = open(slave_name.as_str(), OFlag::O_RDWR, Mode::empty())?;
        Ok(slave_fd)
    }

    fn get_file_handle(&self) -> Result<File> {
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
            let string = unsafe { CStr::from_ptr(buf.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Ok(string);
        }
        _ => Err(Error::last()),
    }
}

fn redirect_std_streams(fd: RawFd) -> Result<()> {
    // If fildes2 is already a valid open file descriptor, it shall be closed first

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
    let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty());
    match fd {
        Ok(fd) => {
            close(fd)?;
        }
        Err(Error::Sys(Errno::ENXIO)) => {
            // Sometimes we get ENXIO right here which 'probably' means
            // that we has been already disconnected from controlling tty.
            // Specifically it was discovered on ubuntu-latest Github CI platform.
        }
        Err(err) => return Err(err),
    }

    setsid()?;

    // Verify we are disconnected from controlling tty by attempting to open
    // it again.  We expect that OSError of ENXIO should always be raised.
    let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty());
    match fd {
        Err(Error::Sys(Errno::ENXIO)) => {} // ok
        Ok(fd) => {
            close(fd)?;
            return Err(Error::UnsupportedOperation);
        }
        Err(_) => return Err(Error::UnsupportedOperation),
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
