use nix::{
    fcntl::{fcntl, FcntlArg, OFlag},
    Result,
};
use std::{io, os::unix::prelude::RawFd};

pub fn make_non_blocking(fd: RawFd) -> Result<()> {
    _make_non_blocking(fd, true)
}

pub fn make_blocking(fd: RawFd) -> Result<()> {
    _make_non_blocking(fd, false)
}

fn _make_non_blocking(fd: RawFd, blocking: bool) -> Result<()> {
    let opt = fcntl(fd, FcntlArg::F_GETFL)?;
    let mut opt = OFlag::from_bits_truncate(opt);
    opt.set(OFlag::O_NONBLOCK, blocking);
    fcntl(fd, FcntlArg::F_SETFL(opt))?;
    Ok(())
}

pub fn nix_error_to_io(err: nix::Error) -> io::Error {
    match err.as_errno() {
        Some(code) => io::Error::from_raw_os_error(code as _),
        None => io::Error::new(
            io::ErrorKind::Other,
            "Unexpected error type conversion from nix to io",
        ),
    }
}
