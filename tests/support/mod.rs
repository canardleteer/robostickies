use std::{io, process::Output, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    time::timeout,
};

pub const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

pub trait CommandOutput {
    async fn bounded_output(&mut self) -> io::Result<Output>;
}

impl CommandOutput for Command {
    async fn bounded_output(&mut self) -> io::Result<Output> {
        let description = format!("{self:?}");
        let mut child = self
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        collect(&mut child, None, PROCESS_TIMEOUT)
            .await
            .map_err(|error| io::Error::new(error.kind(), format!("{description}: {error}")))
    }
}

/// Drain both pipes while waiting and supplying stdin; the deadline includes all I/O.
/// On failure, terminate and reap the child before returning, retaining kill-on-drop
/// as a fallback if the OS cannot finish cleanup within the separate deadline.
pub async fn collect(
    child: &mut Child,
    input: Option<&[u8]>,
    limit: Duration,
) -> io::Result<Output> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();
    let result = timeout(limit, async {
        let read_stdout = async {
            let mut bytes = vec![];
            if let Some(mut pipe) = stdout {
                pipe.read_to_end(&mut bytes).await?;
            }
            Ok::<_, io::Error>(bytes)
        };
        let read_stderr = async {
            let mut bytes = vec![];
            if let Some(mut pipe) = stderr {
                pipe.read_to_end(&mut bytes).await?;
            }
            Ok::<_, io::Error>(bytes)
        };
        let write_stdin = async {
            if let Some(mut pipe) = stdin {
                if let Some(bytes) = input {
                    pipe.write_all(bytes).await?;
                }
                pipe.shutdown().await?;
            }
            Ok::<_, io::Error>(())
        };
        let (status, stdout, stderr, ()) =
            tokio::try_join!(child.wait(), read_stdout, read_stderr, write_stdin)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
    .await
    .unwrap_or_else(|_| {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "subprocess deadline exceeded",
        ))
    });
    if result.is_err() {
        timeout(CLEANUP_TIMEOUT, async {
            if child.try_wait()?.is_none() {
                child.start_kill()?;
            }
            child.wait().await?;
            Ok::<_, io::Error>(())
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "subprocess cleanup deadline exceeded",
            )
        })??;
    }
    result
}
