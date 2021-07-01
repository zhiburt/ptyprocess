#[cfg(feature = "sync")]
pub type Stream = sync_stream::Stream;
#[cfg(feature = "async")]
pub type Stream = async_stream::AsyncStream;

#[cfg(feature = "sync")]
mod sync_stream {
    use super::*;
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
}

#[cfg(feature = "async")]
mod async_stream {
    use super::*;
    use futures_lite::{AsyncRead, AsyncWrite};
    use std::{
        fs::File,
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    #[derive(Debug)]
    pub struct AsyncStream {
        inner: async_fs::File,
    }

    impl AsyncStream {
        pub fn new(file: File) -> Self {
            let file = async_fs::File::from(file);
            Self { inner: file }
        }
    }

    impl AsyncRead for AsyncStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            let future =
                <async_fs::File as AsyncRead>::poll_read(Pin::new(&mut self.inner), cx, buf);
            match future {
                Poll::Ready(Err(ref err)) if has_reached_end_of_sdtout(err) => Poll::Ready(Ok(0)),
                _ => future,
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
}

fn has_reached_end_of_sdtout(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Other && err.raw_os_error() == Some(5)
}
