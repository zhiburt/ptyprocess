/// Stream represent a IO stream.
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::{io::AsRawFd, prelude::RawFd},
};

/// Stream represent a IO stream.
#[derive(Debug)]
pub struct Stream {
    inner: File,
}

impl Stream {
    /// The function returns a new Stream from a file.
    pub fn new(file: File) -> Self {
        Self { inner: file }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.inner.write_vectored(bufs)
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buf) {
            Err(ref err) if has_reached_end_of_sdtout(err) => Ok(0),
            result => result,
        }
    }
}

impl AsRawFd for Stream {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

/// PTY may doesn't have anything to read but the process is not DEAD,
/// and this erorr may be returned.  
fn has_reached_end_of_sdtout(err: &std::io::Error) -> bool {
    // We don't match `err.kind()` because on stable we would expect `Other` but for those who uses nightly
    // we would need to expect `Uncategorized` behind `#![feature(io_error_uncategorized)]` unstable feature.
    // https://doc.rust-lang.org/beta/unstable-book/library-features/io-error-uncategorized.html
    // https://doc.rust-lang.org/nightly/std/io/struct.Error.html#method.kind
    //
    // But we can't use a cfg!() currently to determine if a unstable feature is turned on.
    // https://stackoverflow.com/questions/67454353/how-to-detect-if-an-unstable-feature-is-enabled
    //
    // So we match only errno code.
    err.raw_os_error() == Some(5)
}
