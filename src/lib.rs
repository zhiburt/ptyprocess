//! Ptyprocess library provides an interface for a PTY/TTY communications.
//!
//! ```
//! use ptyprocess::PtyProcess;
//! use std::process::Command;
//! use std::io::{Read, Write};
//!
//! let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");
//!
//! process.write_all(b"hello cat\n").expect("failed write");
//! let mut buf = vec![0; 128];
//! let size = process.read(&mut buf).expect("failed read");
//! assert_eq!(&buf[..size], b"hello cat\r\n");
//!
//! let sucess = process.exit(true).expect("failed to exit");
//!
//! assert_eq!(sucess, true);
//! ```

mod control_code;
mod process;
mod stream;

pub use crate::control_code::ControlCode;
pub use crate::process::PtyProcess;

pub use nix::sys::signal::Signal;
pub use nix::sys::wait::WaitStatus;
