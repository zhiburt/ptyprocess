//! A library provides an interface for a unix [PTY/TTY](https://en.wikipedia.org/wiki/Pseudoterminal).
//!
//! It aims to work on all major Unix variants.
//!
//! The library was developed as a backend for a https://github.com/zhiburt/expectrl.
//! If you're interested in a high level operations may you'd better take a look at `zhiburt/expectrl`.
//!
//! ## Usage
//!
//! ```rust
//! use ptyprocess::PtyProcess;
//! use std::process::Command;
//! use std::io::{BufRead, Write, BufReader};
//!
//! // spawn a cat process
//! let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");
//!
//! // create a communication stream
//! let mut stream = process.get_raw_handle().expect("failed to create a stream");
//!
//! // send a message to process
//! writeln!(stream, "Hello cat").expect("failed to write to a stream");
//!
//! // read a line from the stream
//! let mut reader = BufReader::new(stream);
//! let mut buf = String::new();
//! reader.read_line(&mut buf).expect("failed to read a process output");
//!
//! println!("line={}", buf);
//!
//! // stop the process
//! assert!(process.exit(true).expect("failed to stop the process"))
//! ```

pub mod stream;

pub use nix::errno;
pub use nix::sys::signal::Signal;
pub use nix::sys::wait::WaitStatus;
pub use nix::Error;

use nix::fcntl::{fcntl, open, FcntlArg, FdFlag, OFlag};
use nix::libc::{self, winsize, STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use nix::pty::PtyMaster;
use nix::pty::{grantpt, posix_openpt, unlockpt};
use nix::sys::stat::Mode;
use nix::sys::wait::{self, waitpid};
use nix::sys::{signal, termios};
use nix::unistd::{
    self, close, dup, dup2, fork, isatty, pipe, setsid, sysconf, write, ForkResult, Pid, SysconfVar,
};
use nix::{ioctl_write_ptr_bad, Result};
use signal::Signal::SIGKILL;
use std::fs::File;
use std::os::unix::prelude::{AsRawFd, CommandExt, FromRawFd, RawFd};
use std::process::{self, Command};
use std::thread;
use std::time::{self, Duration};
use stream::Stream;
use termios::SpecialCharacterIndices;

const DEFAULT_TERM_COLS: u16 = 80;
const DEFAULT_TERM_ROWS: u16 = 24;

const DEFAULT_VEOF_CHAR: u8 = 0x4; // ^D
const DEFAULT_INTR_CHAR: u8 = 0x3; // ^C

const DEFAULT_TERMINATE_DELAY: Duration = Duration::from_millis(100);

/// PtyProcess controls a spawned process and communication with this.
///
/// It implements [std::io::Read] and [std::io::Write] to communicate with
/// a child.
///
/// ```no_run,ignore
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
    eof_char: u8,
    intr_char: u8,
    terminate_delay: Duration,
}

impl PtyProcess {
    /// Spawns a child process and create a [PtyProcess].
    ///
    /// ```no_run
    ///   # use std::process::Command;
    ///   # use ptyprocess::PtyProcess;
    ///     let proc = PtyProcess::spawn(Command::new("bash"));
    /// ```
    pub fn spawn(mut command: Command) -> Result<Self> {
        let master = Master::open()?;
        master.grant_slave_access()?;
        master.unlock_slave()?;

        // handle errors in child executions by pipe
        let (exec_err_pipe_r, exec_err_pipe_w) = pipe()?;

        let fork = unsafe { fork()? };
        match fork {
            ForkResult::Child => {
                let err = || -> Result<()> {
                    make_controlling_tty(&master)?;

                    let slave_fd = master.get_slave_fd()?;
                    redirect_std_streams(slave_fd)?;

                    set_echo(STDIN_FILENO, false)?;
                    set_term_size(STDIN_FILENO, DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                    // Do not allow child to inherit open file descriptors from parent
                    close_all_descriptors(&[
                        0,
                        1,
                        2,
                        slave_fd,
                        exec_err_pipe_w,
                        exec_err_pipe_r,
                        master.as_raw_fd(),
                    ])?;

                    close(slave_fd)?;
                    close(exec_err_pipe_r)?;
                    drop(master);

                    // close pipe on sucessfull exec
                    fcntl(exec_err_pipe_w, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;

                    let _ = command.exec();
                    Err(Error::last())
                }()
                .unwrap_err();

                let code = err as i32;

                // Intentionally ignoring errors to exit the process properly
                let _ = write(exec_err_pipe_w, &code.to_be_bytes());
                let _ = close(exec_err_pipe_w);

                process::exit(code);
            }
            ForkResult::Parent { child } => {
                close(exec_err_pipe_w)?;

                let mut pipe_buf = [0u8; 4];
                unistd::read(exec_err_pipe_r, &mut pipe_buf)?;
                close(exec_err_pipe_r)?;
                let code = i32::from_be_bytes(pipe_buf);
                if code != 0 {
                    return Err(errno::from_i32(code));
                }

                // Some systems may work in this way? (not sure)
                // that we need to set a terminal size in a parent.
                set_term_size(master.as_raw_fd(), DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS)?;

                let eof_char = get_eof_char();
                let intr_char = get_intr_char();

                Ok(Self {
                    master,
                    child_pid: child,
                    eof_char,
                    intr_char,
                    terminate_delay: DEFAULT_TERMINATE_DELAY,
                })
            }
        }
    }

    /// Returns a pid of a child process
    pub fn pid(&self) -> Pid {
        self.child_pid
    }

    /// Returns a file representation of a PTY, which can be used
    /// to communicate with a spawned process.
    ///
    /// The file behaivor is platform dependent.
    ///
    /// # Safety
    ///
    /// Be carefull changing a descriptors inner state (e.g `fcntl`)
    /// because it affects all structures which use it.
    ///
    /// Be carefull using this method in async mode.
    /// Because descriptor is set to a non-blocking mode will affect all dublicated descriptors
    /// which may be unexpected.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ptyprocess::PtyProcess;
    /// use std::{process::Command, io::{BufReader, LineWriter}};
    ///
    /// let mut process = PtyProcess::spawn(Command::new("cat")).unwrap();
    /// let pty = process.get_raw_handle().unwrap();
    /// let mut writer = LineWriter::new(&pty);
    /// let mut reader = BufReader::new(&pty);
    /// ```
    pub fn get_raw_handle(&self) -> Result<File> {
        self.master.get_file_handle()
    }

    /// Returns a stream representation of a PTY.
    /// Which can be used to communicate with a spawned process.
    ///
    /// It differs from [Self::get_raw_handle] because it is
    /// platform independent.
    pub fn get_pty_stream(&self) -> Result<Stream> {
        self.get_raw_handle().map(Stream::new)
    }

    /// Get a end of file character if set or a default.
    pub fn get_eof_char(&self) -> u8 {
        self.eof_char
    }

    /// Get a interapt character if set or a default.
    pub fn get_intr_char(&self) -> u8 {
        self.intr_char
    }

    /// Get window size of a terminal.
    ///
    /// Default size is 80x24.
    pub fn get_window_size(&self) -> Result<(u16, u16)> {
        get_term_size(self.master.as_raw_fd())
    }

    /// Sets a terminal size.
    pub fn set_window_size(&mut self, cols: u16, rows: u16) -> Result<()> {
        set_term_size(self.master.as_raw_fd(), cols, rows)
    }

    /// The function returns true if an echo setting is setup.
    pub fn get_echo(&self) -> Result<bool> {
        termios::tcgetattr(self.master.as_raw_fd())
            .map(|flags| flags.local_flags.contains(termios::LocalFlags::ECHO))
    }

    /// Sets a echo setting for a terminal
    pub fn set_echo(&mut self, on: bool, timeout: Option<Duration>) -> Result<bool> {
        set_echo(self.master.as_raw_fd(), on)?;
        self.wait_echo(on, timeout)
    }

    /// Returns true if a underline `fd` connected with a TTY.
    pub fn isatty(&self) -> Result<bool> {
        isatty(self.master.as_raw_fd())
    }

    /// Set the pty process's terminate approach delay.
    pub fn set_terminate_delay(&mut self, terminate_approach_delay: Duration) {
        self.terminate_delay = terminate_approach_delay;
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

    /// Signal is an alias to [PtyProcess::kill].
    ///
    /// [PtyProcess::kill]: struct.PtyProcess.html#method.kill
    pub fn signal(&mut self, signal: signal::Signal) -> Result<()> {
        self.kill(signal)
    }

    /// Wait blocks until a child process exits.
    ///
    /// It returns a error if the child was DEAD or not exist
    /// at the time of a call.
    ///
    /// If you need to verify that a process is dead in non-blocking way you can use
    /// [is_alive] method.
    ///
    /// [is_alive]: struct.PtyProcess.html#method.is_alive
    pub fn wait(&self) -> Result<WaitStatus> {
        waitpid(self.child_pid, None)
    }

    /// Checks if a process is still exists.
    ///
    /// It's a non blocking operation.
    ///
    /// Keep in mind that after calling this method process might be marked as DEAD by kernel,
    /// because a check of its status.
    /// Therefore second call to [Self::status] or [Self::is_alive] might return a different status.
    pub fn is_alive(&self) -> Result<bool> {
        let status = self.status();
        match status {
            Ok(status) if status == WaitStatus::StillAlive => Ok(true),
            Ok(_) | Err(Error::ECHILD) | Err(Error::ESRCH) => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Try to force a child to terminate.
    ///
    /// This returns true if the child was terminated. and returns false if the
    /// child could not be terminated.
    ///
    /// It makes 4 tries getting more thorough.
    ///
    /// 1. SIGHUP
    /// 2. SIGCONT
    /// 3. SIGINT
    /// 4. SIGTERM
    ///
    /// If "force" is `true` then moves onto SIGKILL.
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
        thread::sleep(self.terminate_delay);

        self.is_alive().map(|is_alive| !is_alive)
    }

    fn wait_echo(&self, on: bool, timeout: Option<Duration>) -> Result<bool> {
        let now = time::Instant::now();
        while timeout.is_none() || now.elapsed() < timeout.unwrap() {
            if on == self.get_echo()? {
                return Ok(true);
            }

            thread::sleep(Duration::from_millis(100));
        }

        Ok(false)
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        if let Ok(WaitStatus::StillAlive) = self.status() {
            self.exit(true).unwrap();
        }
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

    #[cfg(not(target_os = "freebsd"))]
    fn get_slave_fd(&self) -> Result<RawFd> {
        let slave_name = self.get_slave_name()?;
        let slave_fd = open(
            slave_name.as_str(),
            OFlag::O_RDWR | OFlag::O_NOCTTY,
            Mode::empty(),
        )?;
        Ok(slave_fd)
    }

    #[cfg(target_os = "freebsd")]
    fn get_slave_fd(&self) -> Result<RawFd> {
        let slave_name = self.get_slave_name()?;
        let slave_fd = open(
            format!("/dev/{}", slave_name.as_str()).as_str(),
            OFlag::O_RDWR | OFlag::O_NOCTTY,
            Mode::empty(),
        )?;
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

#[cfg(target_os = "linux")]
fn get_slave_name(fd: &PtyMaster) -> Result<String> {
    nix::pty::ptsname_r(fd)
}

#[cfg(target_os = "freebsd")]
fn get_slave_name(fd: &PtyMaster) -> Result<String> {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::os::unix::prelude::AsRawFd;

    let fd = fd.as_raw_fd();

    if !isptmaster(fd)? {
        // never reached according current implementation of isptmaster
        return Err(nix::Error::Sys(Errno::EINVAL));
    }

    // todo: Need to determine the correct size via some contstant like SPECNAMELEN in <sys/filio.h>
    let mut buf: [c_char; 128] = [0; 128];

    let _ = fdevname_r(fd, &mut buf)?;

    // todo: determine how CStr::from_ptr handles not NUL terminated string.
    let string = unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned();

    return Ok(string);
}

// https://github.com/freebsd/freebsd-src/blob/main/lib/libc/stdlib/ptsname.c#L52
#[cfg(target_os = "freebsd")]
fn isptmaster(fd: RawFd) -> Result<bool> {
    use nix::libc::ioctl;
    use nix::libc::TIOCPTMASTER;
    match unsafe { ioctl(fd, TIOCPTMASTER as u64, 0) } {
        0 => Ok(true),
        _ => Err(Error::last()),
    }
}

/* automatically generated by rust-bindgen 0.59.1 */
// bindgen filio.h --allowlist-type fiodgname_arg -o bindings.rs
// it may be worth to use a build.rs if we will need more FFI structures.
#[cfg(target_os = "freebsd")]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct fiodgname_arg {
    pub len: ::std::os::raw::c_int,
    pub buf: *mut ::std::os::raw::c_void,
}

// https://github.com/freebsd/freebsd-src/blob/6ae38ab45396edaea26b4725e0c7db8cffa5f208/lib/libc/gen/fdevname.c#L39
#[cfg(target_os = "freebsd")]
fn fdevname_r(fd: RawFd, buf: &mut [std::os::raw::c_char]) -> Result<()> {
    use nix::libc::{ioctl, FIODGNAME};

    nix::ioctl_read_bad!(_ioctl_fiodgname, FIODGNAME, fiodgname_arg);

    let mut fgn = fiodgname_arg {
        len: buf.len() as i32,
        buf: buf.as_mut_ptr() as *mut ::std::os::raw::c_void,
    };

    let _ = unsafe { _ioctl_fiodgname(fd, &mut fgn) }?;

    Ok(())
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

    termios::tcsetattr(fd, termios::SetArg::TCSANOW, &flags)?;
    Ok(())
}

pub fn set_raw(fd: RawFd) -> Result<()> {
    let mut flags = termios::tcgetattr(fd)?;

    #[cfg(not(target_os = "macos"))]
    {
        termios::cfmakeraw(&mut flags);
    }
    #[cfg(target_os = "macos")]
    {
        // implementation is taken from https://github.com/python/cpython/blob/3.9/Lib/tty.py
        use nix::libc::{VMIN, VTIME};
        use termios::ControlFlags;
        use termios::InputFlags;
        use termios::LocalFlags;
        use termios::OutputFlags;

        flags.input_flags &= !(InputFlags::BRKINT
            | InputFlags::ICRNL
            | InputFlags::INPCK
            | InputFlags::ISTRIP
            | InputFlags::IXON);
        flags.output_flags &= !OutputFlags::OPOST;
        flags.control_flags &= !(ControlFlags::CSIZE | ControlFlags::PARENB);
        flags.control_flags |= ControlFlags::CS8;
        flags.local_flags &=
            !(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG);
        flags.control_chars[VMIN] = 1;
        flags.control_chars[VTIME] = 0;
    }

    termios::tcsetattr(fd, termios::SetArg::TCSANOW, &flags)?;
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

fn make_controlling_tty(ptm: &Master) -> Result<()> {
    #[cfg(not(any(target_os = "freebsd", target_os = "macos")))]
    {
        let pts_name = ptm.get_slave_name()?;
        // https://github.com/pexpect/ptyprocess/blob/c69450d50fbd7e8270785a0552484182f486092f/ptyprocess/_fork_pty.py

        // Disconnect from controlling tty, if any
        //
        // it may be a simmilar call to ioctl TIOCNOTTY
        // https://man7.org/linux/man-pages/man4/tty_ioctl.4.html
        let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty());
        match fd {
            Ok(fd) => {
                close(fd)?;
            }
            Err(Error::ENXIO) => {
                // Sometimes we get ENXIO right here which 'probably' means
                // that we has been already disconnected from controlling tty.
                // Specifically it was discovered on ubuntu-latest Github CI platform.
            }
            Err(err) => return Err(err),
        }

        // setsid() will remove the controlling tty. Also the ioctl TIOCNOTTY does this.
        // https://www.win.tue.nl/~aeb/linux/lk/lk-10.html
        setsid()?;

        // Verify we are disconnected from controlling tty by attempting to open
        // it again.  We expect that OSError of ENXIO should always be raised.
        let fd = open("/dev/tty", OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty());
        match fd {
            Err(Error::ENXIO) => {} // ok
            Ok(fd) => {
                close(fd)?;
                return Err(Error::ENOTSUP);
            }
            Err(_) => return Err(Error::ENOTSUP),
        }

        // Verify we can open child pty.
        let fd = open(pts_name.as_str(), OFlag::O_RDWR, Mode::empty())?;
        close(fd)?;

        // Verify we now have a controlling tty.
        let fd = open("/dev/tty", OFlag::O_WRONLY, Mode::empty())?;
        close(fd)?;
    }

    #[cfg(any(target_os = "freebsd", target_os = "macos"))]
    {
        let pts_fd = ptm.get_slave_fd()?;

        // https://docs.freebsd.org/44doc/smm/01.setup/paper-3.html
        setsid()?;

        use nix::libc::ioctl;
        use nix::libc::TIOCSCTTY;
        match unsafe { ioctl(pts_fd, TIOCSCTTY as u64, 0) } {
            0 => {}
            _ => return Err(Error::last()),
        }
    }

    Ok(())
}

// Except is used for cases like double free memory
fn close_all_descriptors(except: &[RawFd]) -> Result<()> {
    // On linux could be used getrlimit(RLIMIT_NOFILE, rlim) interface
    let max_open_fds = sysconf(SysconfVar::OPEN_MAX)?.unwrap() as i32;
    (0..max_open_fds)
        .filter(|fd| !except.contains(fd))
        .for_each(|fd| {
            // We don't handle errors intentionally,
            // because it will be hard to determine which descriptors closed already.
            let _ = close(fd);
        });

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

        let expected_path = if cfg!(target_os = "freebsd") {
            "pts/"
        } else if cfg!(target_os = "macos") {
            "/dev/ttys"
        } else {
            "/dev/pts/"
        };

        if !slavename.starts_with(expected_path) {
            assert_eq!(expected_path, slavename);
        }

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
