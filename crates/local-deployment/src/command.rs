use executors::executors::ChildHandle;
use services::services::container::ContainerError;

pub async fn kill_process_group(child: &mut ChildHandle) -> Result<(), ContainerError> {
    match child {
        ChildHandle::Group(child) => utils::process::kill_process_group(child)
            .await
            .map_err(ContainerError::KillFailed),
        ChildHandle::Pty { child: pty_child, .. } => {
            if let Some(pid) = pty_child.process_id() {
                let mut tw = || {
                    pty_child.try_wait().map(|opt| {
                        opt.map(|es| {
                            #[cfg(unix)]
                            {
                                std::os::unix::process::ExitStatusExt::from_raw(
                                    es.exit_code() as i32,
                                )
                            }
                            #[cfg(not(unix))]
                            {
                                let _ = es;
                                std::process::ExitStatus::default()
                            }
                        })
                    })
                };
                utils::process::kill_process_group_by_pid(pid, &mut tw)
                    .await
                    .map_err(ContainerError::KillFailed)?;
            }
            let _ = pty_child.kill();
            let _ = pty_child.wait();
            Ok(())
        }
    }
}
