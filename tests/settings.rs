use ptyprocess::PtyProcess;
use std::{process::Command, time::Duration};

#[test]
fn default_win_size() {
    let proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert_eq!(proc.get_window_size().unwrap(), (80, 24));
}

#[test]
fn set_win_size() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    proc.set_window_size(100, 200).unwrap();

    assert_eq!(proc.get_window_size().unwrap(), (100, 200));
}

#[test]
fn default_echo() {
    let proc = PtyProcess::spawn(Command::new("cat")).unwrap();
    assert!(!proc.get_echo().unwrap());
}

#[test]
fn set_echo() {
    let mut proc = PtyProcess::spawn(Command::new("cat")).unwrap();

    assert!(proc.isatty().unwrap());

    let is_set = proc.set_echo(true, Some(Duration::from_millis(500))).unwrap();
    
    assert!(is_set);
    assert!(proc.get_echo().unwrap());
}
