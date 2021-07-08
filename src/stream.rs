#[cfg(feature = "sync")]
pub type Stream = sync_stream::Stream;
#[cfg(feature = "async")]
pub type Stream = async_stream::AsyncStream;

#[cfg(feature = "sync")]
mod sync_stream {
    use crate::util::{make_blocking, make_non_blocking, nix_error_to_io};

    use super::*;
    use std::{
        fs::File,
        io::{self, BufRead, BufReader, Read, Write},
        os::unix::prelude::AsRawFd,
    };

    #[derive(Debug)]
    pub struct Stream {
        inner: File,
        reader: BufReader<File>,
    }

    impl Stream {
        pub fn new(file: File) -> Self {
            let copy_file = file
                .try_clone()
                .expect("It's ok to clone fd as it will be just DUPed");
            let reader = BufReader::new(copy_file);
            Self {
                inner: file,
                reader,
            }
        }

        /// Try to read in a non-blocking mode.
        ///
        /// It returns Ok(None) if there's nothing to read.
        /// Otherwise it operates as general `std::io::Read` interface.
        pub fn try_read(&mut self, mut buf: &mut [u8]) -> io::Result<Option<usize>> {
            let fd = self.inner.as_raw_fd();
            make_non_blocking(fd).map_err(nix_error_to_io)?;

            let result = match self.read(&mut buf) {
                Ok(n) => Ok(Some(n)),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
                Err(err) => Err(err),
            };

            // As file is DUPed changes in one descriptor affects all ones
            // so we need to make blocking file after we finished.
            make_blocking(fd).map_err(nix_error_to_io)?;

            result
        }

        /// Try to read a byte in a non-blocking mode.
        ///
        /// Returns:
        ///     - `None` if there's nothing to read.
        ///     - `Some(None)` on eof.
        ///     - `Some(Some(byte))` on sucessfull call.
        ///
        /// For more information look at [`try_read`].
        ///
        /// [`try_read`]: struct.PtyProcess.html#method.try_read
        pub fn try_read_byte(&mut self) -> io::Result<Option<Option<u8>>> {
            let mut buf = [0; 1];
            match self.try_read(&mut buf)? {
                Some(1) => Ok(Some(Some(buf[0]))),
                Some(0) => Ok(Some(None)),
                None => Ok(None),
                Some(_) => unreachable!(),
            }
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
            match self.reader.read(buf) {
                Err(ref err) if has_reached_end_of_sdtout(err) => Ok(0),
                result => result,
            }
        }
    }

    impl BufRead for Stream {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            self.reader.fill_buf()
        }

        fn consume(&mut self, amt: usize) {
            self.reader.consume(amt)
        }
    }
}

#[cfg(feature = "async")]
mod async_stream {
    use super::*;
    use crate::util::{make_blocking, make_non_blocking, nix_error_to_io};
    use futures_lite::{io::BufReader, AsyncBufRead, AsyncRead, AsyncReadExt, AsyncWrite};
    use std::{
        fs::File,
        io,
        os::unix::prelude::AsRawFd,
        pin::Pin,
        task::{Context, Poll},
    };

    #[pin_project::pin_project]
    #[derive(Debug)]
    pub struct AsyncStream {
        inner: async_fs::File,
        reader: BufReader<async_fs::File>,
    }

    impl AsyncStream {
        pub fn new(file: File) -> Self {
            let cloned = file.try_clone().unwrap();
            let file = async_fs::File::from(file);
            let file_cloned = async_fs::File::from(cloned);
            let reader = BufReader::new(file_cloned);

            Self {
                inner: file,
                reader,
            }
        }

        /// Try to read in a non-blocking mode.
        ///
        /// It returns Ok(None) if there's nothing to read.
        /// Otherwise it operates as general `std::io::Read` interface.
        pub async fn try_read(&mut self, mut buf: &mut [u8]) -> io::Result<Option<usize>> {
            // future::poll_once was testing but it doesn't work why?
            // let a = future::poll_once(self.reader.read(buf)).await;
            // match a {
            //     Some(a) => match a {
            //         Ok(n) => Ok(Some(n)),
            //         Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            //         Err(err) => Err(err),
            //     },
            //     None => Ok(None),
            // }

            let fd = self.inner.as_raw_fd();
            make_non_blocking(fd).map_err(nix_error_to_io)?;

            let result = match self.read(&mut buf).await {
                Ok(n) => Ok(Some(n)),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
                Err(err) => Err(err),
            };

            // As file is DUPed changes in one descriptor affects all ones
            // so we need to make blocking file after we finished.
            make_blocking(fd).map_err(nix_error_to_io)?;

            result
        }

        /// Try to read a byte in a non-blocking mode.
        ///
        /// Returns:
        ///     - `None` if there's nothing to read.
        ///     - `Some(None)` on eof.
        ///     - `Some(Some(byte))` on sucessfull call.
        ///
        /// For more information look at [`try_read`].
        ///
        /// [`try_read`]: struct.PtyProcess.html#method.try_read
        pub async fn try_read_byte(&mut self) -> io::Result<Option<Option<u8>>> {
            let mut buf = [0; 1];
            match self.try_read(&mut buf).await? {
                Some(1) => Ok(Some(Some(buf[0]))),
                Some(0) => Ok(Some(None)),
                None => Ok(None),
                Some(_) => unreachable!(),
            }
        }
    }

    impl AsyncWrite for AsyncStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            <async_fs::File as AsyncWrite>::poll_write(Pin::new(&mut self.inner), cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            <async_fs::File as AsyncWrite>::poll_flush(Pin::new(&mut self.inner), cx)
        }

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            <async_fs::File as AsyncWrite>::poll_close(Pin::new(&mut self.inner), cx)
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[io::IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            <async_fs::File as AsyncWrite>::poll_write_vectored(Pin::new(&mut self.inner), cx, bufs)
        }
    }

    impl AsyncRead for AsyncStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let future = Pin::new(&mut self.reader).poll_read(cx, buf);
            match future {
                Poll::Ready(Err(ref err)) if has_reached_end_of_sdtout(err) => Poll::Ready(Ok(0)),
                _ => future,
            }
        }
    }

    impl AsyncBufRead for AsyncStream {
        fn poll_fill_buf<'a>(
            self: Pin<&'a mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<&'a [u8]>> {
            // pin_project is used only for this function.
            // the solution was found in the original implementation of BufReader.
            let this = self.project();
            Pin::new(this.reader).poll_fill_buf(cx)
        }

        fn consume(mut self: Pin<&mut Self>, amt: usize) {
            Pin::new(&mut self.reader).consume(amt)
        }
    }
}

pub fn has_reached_end_of_sdtout(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Other && err.raw_os_error() == Some(5)
}
