//! Binary-test registration only: the custom harness also imports the runner module.
use crate::runner::output::write_bounded;
use std::io;

#[test]
fn partial_writes_preserve_bytes_across_backpressure_and_interruption() {
    let mut written = Vec::new();
    let mut calls = 0;
    let mut progress = 0;
    write_bounded(
        b"a\xffbc",
        |bytes| {
            calls += 1;
            match calls {
                1 => Err(io::ErrorKind::WouldBlock.into()),
                2 => Err(io::ErrorKind::Interrupted.into()),
                _ => {
                    written.push(bytes[0]);
                    Ok(1)
                }
            }
        },
        |made_progress| {
            progress += usize::from(made_progress);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(written, b"a\xffbc");
    assert_eq!(progress, 4);
    assert_eq!(calls, 6);
}

#[test]
fn both_zero_progress_and_would_block_remain_budgeted() {
    for zero in [true, false] {
        let mut attempts = 0;
        let result = write_bounded(
            b"x",
            |_| {
                if zero {
                    Ok(0)
                } else {
                    Err(io::ErrorKind::WouldBlock.into())
                }
            },
            |progress| {
                assert!(!progress);
                attempts += 1;
                if attempts == 4 {
                    Err(io::ErrorKind::TimedOut.into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(attempts, 4);
    }
}

#[test]
fn cancelled_budget_prevents_another_write() {
    let result = write_bounded(
        b"x",
        |_| panic!("Cancelled output must not be written"),
        |_| Err(io::ErrorKind::Interrupted.into()),
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
}

#[test]
fn broken_sink_is_not_retried_or_reported_as_progress() {
    let mut calls = 0;
    let result = write_bounded(
        b"x",
        |_| {
            calls += 1;
            Err(io::ErrorKind::BrokenPipe.into())
        },
        |progress| {
            assert!(!progress);
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(calls, 1);
}
