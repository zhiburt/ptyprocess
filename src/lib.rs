//! Ptyprocess library provides an interface for a PTY/TTY.
//!
//! The entry point to the library is a [PtyProcess].
//!
//! The library provides a `sync` and `async` IO operations for communication.
//! To be able to use `async` you must to provide a feature flag `[async]`
//! and turn off default features `default-features = false`.
//!
//! The library was developed as a backend for a https://github.com/zhiburt/expectrl.
//! If you're interested in a high level operations may you'd better take a look at `zhiburt/expectrl`.
//!
//! # Example
//!
//! ```no_run,ignore
//! use ptyprocess::PtyProcess;
//! use std::process::Command;
//! use std::io::{Read, Write};
//!
//! // spawn a cat process
//! let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");
//!
//! // write message to cat.
//! process.write_all(b"hello cat\n").expect("failed to write");
//!
//! // read what cat produced.
//! let mut buf = vec![0; 128];
//! let size = process.read(&mut buf).expect("failed to read");
//! assert_eq!(&buf[..size], b"hello cat\r\n");
//!
//! // stop process
//! let sucess = process.exit(true).expect("failed to exit");
//!
//! assert_eq!(sucess, true);
//! ```
//!
//! # Async
//!
//! ## Example
//!
//! ```no_run,ignore
//! use ptyprocess::PtyProcess;
//! use std::process::Command;
//!
//! // spawns a cat process
//! let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");
//!
//! // sends line to cat
//! process.send_line("hello cat").await.expect("failed writing");
//! ```

mod control_code;
mod process;
pub mod stream;

pub use crate::control_code::ControlCode;
pub use crate::process::PtyProcess;

pub use nix::sys::signal::Signal;
pub use nix::sys::wait::WaitStatus;
pub use nix::Error;
