//! Keep downstream pipe backpressure inside the command's existing time/interrupt budget.
use std::{io::{self, Write}, thread, time::Duration};

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
        Ok(Self { pipe: platform::Pipe::stdout()? })
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

fn write_bounded(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_writes_preserve_bytes_across_backpressure_and_interruption() {
        let mut written = Vec::new();
        let mut calls = 0;
        let mut progress = 0;
        write_bounded(b"a\xffbc", |bytes| {
            calls += 1;
            match calls {
                1 => Err(io::ErrorKind::WouldBlock.into()),
                2 => Err(io::ErrorKind::Interrupted.into()),
                _ => { written.push(bytes[0]); Ok(1) }
            }
        }, |made_progress| { progress += usize::from(made_progress); Ok(()) }).unwrap();
        assert_eq!(written, b"a\xffbc");
        assert_eq!(progress, 4);
        assert_eq!(calls, 6);
    }

    #[test]
    fn both_zero_progress_and_would_block_remain_budgeted() {
        for zero in [true, false] {
            let mut attempts = 0;
            let result = write_bounded(b"x", |_| {
                if zero { Ok(0) } else { Err(io::ErrorKind::WouldBlock.into()) }
            }, |progress| {
                assert!(!progress);
                attempts += 1;
                if attempts == 4 { Err(io::ErrorKind::TimedOut.into()) } else { Ok(()) }
            });
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
            assert_eq!(attempts, 4);
        }
    }

    #[test]
    fn cancelled_budget_prevents_another_write() {
        let result = write_bounded(b"x", |_| panic!("Cancelled output must not be written"),
            |_| Err(io::ErrorKind::Interrupted.into()));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn broken_sink_is_not_retried_or_reported_as_progress() {
        let mut calls = 0;
        let result = write_bounded(b"x", |_| {
            calls += 1;
            Err(io::ErrorKind::BrokenPipe.into())
        }, |progress| { assert!(!progress); Ok(()) });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(calls, 1);
    }
}
