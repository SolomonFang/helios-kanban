use std::{path::Path, time::Duration};

use command_group::AsyncCommandGroup;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
};
use ts_rs::TS;

use crate::{
    command::{CmdOverrides, CommandParts},
    env::ExecutionEnv,
    executors::ExecutorError,
};

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct AcpConfigOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct AcpModelOption {
    pub value: String,
    pub label: String,
    pub variants: Vec<AcpConfigOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
pub struct AcpConfigOptions {
    pub current_model: Option<String>,
    pub models: Vec<AcpModelOption>,
    pub agents: Vec<AcpConfigOption>,
}

pub async fn discover_config_options(
    command_parts: CommandParts,
    current_dir: &Path,
    env: &ExecutionEnv,
    cmd_overrides: &CmdOverrides,
) -> Result<AcpConfigOptions, ExecutorError> {
    let (program_path, args) = command_parts.into_resolved().await?;

    let mut command = Command::new(program_path);
    command
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(current_dir)
        .env("NPM_CONFIG_LOGLEVEL", "error")
        .env("NODE_NO_WARNINGS", "1")
        .args(&args);
    env.clone()
        .with_profile(cmd_overrides)
        .apply_to_command(&mut command);

    let mut child = command.group_spawn()?;
    let stdin = child
        .inner()
        .stdin
        .take()
        .ok_or_else(|| ExecutorError::Io(std::io::Error::other("ACP missing stdin")))?;
    let stdout = child
        .inner()
        .stdout
        .take()
        .ok_or_else(|| ExecutorError::Io(std::io::Error::other("ACP missing stdout")))?;
    if let Some(stderr) = child.inner().stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(_)) = lines.next_line().await {}
        });
    }

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        probe_config_options(stdin, stdout, current_dir),
    )
    .await
    .map_err(|_| {
        ExecutorError::Io(std::io::Error::other(
            "Timed out reading ACP configuration options",
        ))
    });
    let _ = workspace_utils::process::kill_process_group(&mut child).await;
    result?
}

async fn probe_config_options(
    mut stdin: ChildStdin,
    stdout: ChildStdout,
    current_dir: &Path,
) -> Result<AcpConfigOptions, ExecutorError> {
    let mut lines = BufReader::new(stdout).lines();
    let mut next_id = 1;

    request(
        &mut stdin,
        &mut lines,
        &mut next_id,
        "initialize",
        serde_json::json!({
            "protocolVersion": 1,
            "clientCapabilities": {}
        }),
    )
    .await?;

    let session = request(
        &mut stdin,
        &mut lines,
        &mut next_id,
        "session/new",
        serde_json::json!({
            "cwd": current_dir.to_string_lossy(),
            "mcpServers": []
        }),
    )
    .await?;
    let session_id = session
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other(
                "ACP session/new did not return a session ID",
            ))
        })?
        .to_string();
    let initial_options = parse_config_options(&session)?;
    let agents = options_by_id(&initial_options, "mode")
        .map(parse_options)
        .unwrap_or_default();
    let current_model = options_by_id(&initial_options, "model")
        .and_then(|option| option.get("currentValue"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let model_values = options_by_id(&initial_options, "model")
        .map(parse_options)
        .unwrap_or_default();

    let mut models = Vec::with_capacity(model_values.len());
    for model in model_values {
        let updated = request(
            &mut stdin,
            &mut lines,
            &mut next_id,
            "session/set_config_option",
            serde_json::json!({
                "sessionId": session_id,
                "configId": "model",
                "value": model.value,
            }),
        )
        .await?;
        let updated_options = parse_config_options(&updated)?;
        let variants = options_by_id(&updated_options, "effort")
            .or_else(|| options_by_id(&updated_options, "thinking"))
            .map(parse_options)
            .unwrap_or_default()
            .into_iter()
            .filter(|option| option.value != "default")
            .collect();
        models.push(AcpModelOption {
            value: model.value,
            label: model.label,
            variants,
        });
    }

    Ok(AcpConfigOptions {
        current_model,
        models,
        agents,
    })
}

async fn request(
    stdin: &mut ChildStdin,
    lines: &mut tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value, ExecutorError> {
    let id = *next_id;
    *next_id += 1;
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    stdin
        .write_all(request.to_string().as_bytes())
        .await
        .map_err(ExecutorError::Io)?;
    stdin.write_all(b"\n").await.map_err(ExecutorError::Io)?;
    stdin.flush().await.map_err(ExecutorError::Io)?;

    loop {
        let line = lines
            .next_line()
            .await
            .map_err(ExecutorError::Io)?
            .ok_or_else(|| {
                ExecutorError::Io(std::io::Error::other(format!(
                    "OpenCode ACP exited during {method}"
                )))
            })?;
        let message: Value = serde_json::from_str(&line)?;
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(ExecutorError::Io(std::io::Error::other(format!(
                "OpenCode {method} failed: {error}"
            ))));
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
}

fn parse_config_options(value: &Value) -> Result<Vec<&Value>, ExecutorError> {
    value
        .get("configOptions")
        .and_then(Value::as_array)
        .map(|options| options.iter().collect())
        .ok_or_else(|| {
            ExecutorError::Io(std::io::Error::other(
                "OpenCode response did not contain configuration options",
            ))
        })
}

fn options_by_id<'a>(options: &[&'a Value], id: &str) -> Option<&'a Value> {
    options
        .iter()
        .copied()
        .find(|option| option.get("id").and_then(Value::as_str) == Some(id))
}

fn parse_options(value: &Value) -> Vec<AcpConfigOption> {
    value
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(AcpConfigOption {
                value: option.get("value")?.as_str()?.to_string(),
                label: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{options_by_id, parse_config_options, parse_options};

    #[test]
    fn parses_model_and_agent_options() {
        let response = json!({
            "configOptions": [
                {"id":"model","options":[{"value":"provider/model","name":"Model"}]},
                {"id":"mode","options":[{"value":"build","name":"Build"},{"value":"plan","name":"Plan"}]}
            ]
        });
        let options = parse_config_options(&response).unwrap();

        assert_eq!(
            parse_options(options_by_id(&options, "model").unwrap())[0].value,
            "provider/model"
        );
        assert_eq!(
            parse_options(options_by_id(&options, "mode").unwrap())[1].value,
            "plan"
        );
    }
}
