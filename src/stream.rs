use std::{
    fs::File,
    io::{self, Read, Write},
};

#[derive(Debug)]
pub struct Stream {
    inner: File,
}

impl Stream {
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

fn has_reached_end_of_sdtout(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Other && err.raw_os_error() == Some(5)
}
