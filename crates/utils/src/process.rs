use command_group::AsyncGroupChild;
#[cfg(unix)]
use nix::{
    sys::signal::{Signal, killpg},
    unistd::{Pid, getpgid},
};
#[cfg(unix)]
use tokio::time::Duration;

pub async fn kill_process_group(child: &mut AsyncGroupChild) -> std::io::Result<()> {
    // hit the whole process group, not just the leader
    #[cfg(unix)]
    {
        if let Some(pid) = child.inner().id() {
            let pgid = getpgid(Some(Pid::from_raw(pid as i32)))
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            for sig in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGKILL] {
                tracing::info!("Sending {:?} to process group {}", sig, pgid);
                if let Err(e) = killpg(pgid, sig) {
                    tracing::warn!(
                        "Failed to send signal {:?} to process group {}: {}",
                        sig,
                        pgid,
                        e
                    );
                }
                tracing::info!("Waiting 2s for process group {} to exit", pgid);
                tokio::time::sleep(Duration::from_secs(2)).await;
                if child.inner().try_wait()?.is_some() {
                    tracing::info!("Process group {} exited after {:?}", pgid, sig);
                    break;
                }
            }
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(())
}

/// Kill a process group identified by PID, using a provided `try_wait`
/// function to check whether the child has exited.
///
/// On Unix this first sends SIGINT, SIGTERM, SIGKILL at 2 s intervals
/// (checking try_wait after each signal).  After that loop it is up to the
/// caller to call `kill()` / `wait()` on the remaining child.
#[cfg(unix)]
pub async fn kill_process_group_by_pid(
    pid: u32,
    try_wait_fn: &mut (dyn FnMut() -> std::io::Result<Option<std::process::ExitStatus>> + Send),
) -> std::io::Result<()> {
    use nix::{
        sys::signal::{Signal, killpg},
        unistd::{Pid, getpgid},
    };

    let pgid = getpgid(Some(Pid::from_raw(pid as i32)))
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    for sig in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGKILL] {
        tracing::info!("Sending {:?} to process group {}", sig, pgid);
        if let Err(e) = killpg(pgid, sig) {
            tracing::warn!(
                "Failed to send signal {:?} to process group {}: {}",
                sig,
                pgid,
                e
            );
        }
        tracing::info!("Waiting 2s for process group {} to exit", pgid);
        tokio::time::sleep(Duration::from_secs(2)).await;
        if try_wait_fn()?.is_some() {
            tracing::info!("Process group {} exited after {:?}", pgid, sig);
            break;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub async fn kill_process_group_by_pid(
    _pid: u32,
    _try_wait_fn: &mut (dyn FnMut() -> std::io::Result<Option<std::process::ExitStatus>> + Send),
) -> std::io::Result<()> {
    Ok(())
}
