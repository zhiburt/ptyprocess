# ptyprocess [![Build](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml/badge.svg)](https://github.com/zhiburt/ptyprocess/actions/workflows/ci.yml) [![codecov](https://codecov.io/gh/zhiburt/ptyprocess/branch/main/graph/badge.svg?token=QBQLAT904B)](https://codecov.io/gh/zhiburt/ptyprocess) [![Crate](https://img.shields.io/crates/v/ptyprocess)](https://crates.io/crates/ptyprocess) [![docs.rs](https://img.shields.io/docsrs/ptyprocess?color=blue)](https://docs.rs/ptyprocess/0.1.0/ptyprocess/) [![license](https://img.shields.io/github/license/zhiburt/ptyprocess)](./LICENSE.txt)

A library provides an interface for a unix [PTY/TTY](https://en.wikipedia.org/wiki/Pseudoterminal).

It aims to work on all major Unix variants.

The library was developed as a backend for a https://github.com/zhiburt/expectrl.
If you're interested in a high level operations may you'd better take a look at `zhiburt/expectrl`.

## Usage

```rust
use ptyprocess::PtyProcess;
use std::io::{BufRead, BufReader, Result, Write};
use std::process::Command;

fn main() -> Result<()> {
    // spawn a cat process
    let mut process = PtyProcess::spawn(Command::new("cat"))?;

    // create a communication stream
    let mut stream = process.get_raw_handle()?;

    // send a message to process
    writeln!(stream, "Hello cat")?;

    // read a line from the stream
    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;

    println!("line was entered {buf:?}");

    // stop the process
    assert!(process.exit(true)?);

    Ok(())
}
```