//! Keep downstream pipe backpressure inside the command's existing time/interrupt budget.
use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

#[cfg(unix)]
#[path = "output_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "output_windows.rs"]
mod platform;

pub struct Output {
    pipe: Option<platform::Pipe>,
}
impl Output {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            pipe: platform::Pipe::stdout()?,
        })
    }

    pub fn write_chunk(
        &mut self,
        bytes: &[u8],
        mut budget: impl FnMut(bool) -> io::Result<()>,
    ) -> io::Result<()> {
        if let Some(pipe) = self.pipe.as_mut() {
            // This is synchronous nonblocking I/O, not an overlapped-I/O emulation.
            write_bounded(bytes, |remaining| pipe.write(remaining), budget)
        } else {
            // Preserve console encoding and ordinary file behavior. Arbitrary hung devices
            // and blocked status sinks are outside the data-pipe deadline contract.
            budget(false)?;
            let mut output = io::stdout().lock();
            output.write_all(bytes)?;
            output.flush()?;
            budget(true)
        }
    }

    pub fn restore(&mut self) -> io::Result<()> {
        self.pipe.as_mut().map_or(Ok(()), platform::Pipe::restore)
    }
}

pub(crate) fn write_bounded(
    mut bytes: &[u8],
    mut write: impl FnMut(&[u8]) -> io::Result<usize>,
    mut budget: impl FnMut(bool) -> io::Result<()>,
) -> io::Result<()> {
    while !bytes.is_empty() {
        budget(false)?;
        match write(bytes) {
            Ok(0) => thread::sleep(Duration::from_millis(2)),
            Ok(count) if count <= bytes.len() => {
                bytes = &bytes[count..];
                budget(true)?;
            }
            Ok(_) => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
