# ptyprocess [![Build](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml/badge.svg)](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/zhiburt/ptyprocess/branch/main/graph/badge.svg?token=QBQLAT904B)](https://codecov.io/gh/zhiburt/ptyprocess) [![Crate](https://img.shields.io/crates/v/ptyprocess)](https://crates.io/crates/ptyprocess) [![docs.rs](https://img.shields.io/docsrs/ptyprocess?color=blue)](https://docs.rs/ptyprocess/0.1.0/ptyprocess/) [![license](https://img.shields.io/github/license/zhiburt/ptyprocess)](./LICENSE.txt)

A library provides an interface for a PTY/TTY communications.

It has an `sync` and `async` backends. Default is `sync`.
You can turn on an async interface by `async` feature. 

## Usage

```rust
use ptyprocess::PtyProcess;
use std::process::Command;
use std::io::{Read, Write};

fn main() {
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");

    process.write_all(b"hello cat\n").expect("failed to write");

    let mut buf = vec![0; 128];
    let size = process.read(&mut buf).expect("failed to read");
    assert_eq!(&buf[..size], b"hello cat\r\n");

    assert!(process.exit(true).expect("failed toexit"));
}
```

 ### Async
 
It must support most runtimes such (`tokio`, `async-std`, `smol` etc.).

```rust
use ptyprocess::PtyProcess;
use std::process::Command;
use futures_lite::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
fn main() {
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");

    process.write_all(b"hello cat\n").await.expect("failed to write");

    let mut buf = vec![0; 128];
    let size = process.read(&mut buf).await.expect("failed to read");
    assert_eq!(&buf[..size], b"hello cat\r\n");

    assert!(process.exit(true).expect("failed toexit"));
}
```
