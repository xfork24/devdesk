//! Bounded coordinator subprocesses. No shell interpolation or raw external errors.
use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

fn drain(mut input: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    let mut truncated = false;
    while let Ok(n) = input.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let take = n.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&chunk[..take]);
        truncated |= take < n;
    }
    (bytes, truncated)
}

pub(crate) fn capture(program: &str, args: &[String], timeout: Duration) -> Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    capture_command(command, timeout, None)
}
pub(crate) fn capture_split(program: &str, args: &[String], timeout: Duration) -> Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    capture_command_streams(command, timeout, None, 1024 * 1024, true)
}
pub(crate) fn capture_command(
    command: Command,
    timeout: Duration,
    input: Option<Vec<u8>>,
) -> Result<Output> {
    capture_command_limited(command, timeout, input, 1024 * 1024)
}
pub(crate) fn capture_command_limited(
    command: Command,
    timeout: Duration,
    input: Option<Vec<u8>>,
    limit: usize,
) -> Result<Output> {
    capture_command_streams(command, timeout, input, limit, false)
}
fn capture_command_streams(
    mut command: Command,
    timeout: Duration,
    input: Option<Vec<u8>>,
    limit: usize,
    stderr: bool,
) -> Result<Output> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(if stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        // Docker and GitHub CLI use their own credential stores. Ambient model keys are not forwarded.
        .env_remove("OPENAI_API_KEY")
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("ZAI_CODING_CN_API_KEY")
        .env_remove("MINIMAX_CN_API_KEY")
        .env_remove("ZAI_API_KEY")
        .env_remove("MINIMAX_API_KEY")
        .env_remove("OLLAMA_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .spawn()
        .map_err(|_| err("provider_unavailable", "无法启动外部进程"))?;
    let writer = if let Some(mut input) = input {
        let mut stream = child.stdin.take().unwrap();
        Some(thread::spawn(move || {
            use std::io::Write;
            let _ = stream.write_all(&input);
            input.fill(0);
        }))
    } else {
        None
    };
    let output = child.stdout.take().unwrap();
    let reader = thread::spawn(move || drain(output, limit));
    let stderr_reader = child
        .stderr
        .take()
        .map(|stream| thread::spawn(move || drain(stream, limit)));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                kill_group(&mut child);
                let _ = child.wait();
                break None;
            }
        }
    };
    kill_group(&mut child);
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let (stdout, truncated) = reader
        .join()
        .map_err(|_| err("provider_io", "进程输出读取失败"))?;
    let (stderr, stderr_truncated) = stderr_reader
        .map(|r| {
            r.join()
                .map_err(|_| err("provider_io", "进程错误输出读取失败"))
        })
        .transpose()?
        .unwrap_or_default();
    let status = status.ok_or_else(|| {
        err(
            "needs_reconciliation",
            "供应商调用超时或失联；必须查询原操作结果",
        )
    })?;
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr,
        truncated: truncated || stderr_truncated,
    })
}

/// All callers create a new process group before spawn; only that owned group is signalled.
pub(crate) fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn ordinary_commands_discard_stderr_and_evidence_capture_is_explicit_and_bounded() {
        let args = vec!["-c".into(), "printf stdout; printf stderr >&2".into()];
        let ordinary = capture("/bin/sh", &args, Duration::from_secs(2)).unwrap();
        assert_eq!(ordinary.stdout, b"stdout");
        assert!(ordinary.stderr.is_empty());
        let evidence = capture_split("/bin/sh", &args, Duration::from_secs(2)).unwrap();
        assert_eq!(evidence.stderr, b"stderr");
        let mut command = Command::new("/bin/sh");
        command.args(&args);
        let bounded =
            capture_command_streams(command, Duration::from_secs(2), None, 3, true).unwrap();
        assert_eq!(bounded.stdout, b"std");
        assert_eq!(bounded.stderr, b"std");
        assert!(bounded.truncated);
    }
    #[test]
    #[cfg(unix)]
    fn timeout_reaps_owned_descendants_instead_of_blocking_on_stdout() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & wait"]);
        let start = Instant::now();
        let result = capture_command(command, Duration::from_millis(100), None);
        assert_eq!(result.err().unwrap().code, "needs_reconciliation");
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
