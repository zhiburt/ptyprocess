# ptyprocess [![Build](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml/badge.svg)](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/zhiburt/ptyprocess/branch/main/graph/badge.svg?token=QBQLAT904B)](https://codecov.io/gh/zhiburt/ptyprocess) [![Crate](https://img.shields.io/crates/v/ptyprocess)](https://crates.io/crates/ptyprocess) [![docs.rs](https://img.shields.io/docsrs/ptyprocess?color=blue)](https://docs.rs/ptyprocess/0.1.0/ptyprocess/) [![license](https://img.shields.io/github/license/zhiburt/ptyprocess)](./LICENSE.txt)

A library provides an interface for a PTY/TTY.

The library provides a `sync` and `async` IO operations for communication.
To be able to use `async` you must provide a feature flag `[async]`
and turn off default features `default-features = false`.

The library was developed as a backend for a https://github.com/zhiburt/expectrl.
If you're interested in a high level operations may you'd better take a look at `zhiburt/expectrl`.

## Usage

```rust
use ptyprocess::PtyProcess;
use std::process::Command;
use std::io::{Read, Write};

fn main() {
    // spawn a cat process
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");

    // write message to cat.
    process.write_all(b"hello cat\n").expect("failed to write");

    // read what cat produced.
    let mut buf = vec![0; 128];
    let size = process.read(&mut buf).expect("failed to read");
    assert_eq!(&buf[..size], b"hello cat\r\n");

    // stop process
    let sucess = process.exit(true).expect("failed to exit");
    assert_eq!(sucess, true);
}
```

 ## Async

 ```rust
 use ptyprocess::PtyProcess;
 use std::process::Command;

#[tokio::main]
async fn main() {
    // spawns a cat process
    let mut process = PtyProcess::spawn(Command::new("cat")).expect("failed to spawn a process");

    // sends line to cat
    process.send_line("hello cat").await.expect("failed writing");
}
```
