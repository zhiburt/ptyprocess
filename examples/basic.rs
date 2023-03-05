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
