use std::path::PathBuf;

use tokio::process::Command;

use crate::{
    agentcore::tool::ToolError,
    config::schema::{ContainerRuntime, SandboxConfig},
};

#[derive(Debug)]
pub(crate) struct ContainerGuard {
    runtime: String,
    id: String,
}

impl ContainerGuard {
    /// 同路径只读挂载进容器。
    pub async fn create(runtime: &str, image: &str, mounts: &[PathBuf]) -> Result<Self, ToolError> {
        let mut cmd = Command::new(runtime);
        cmd.arg("create").arg("--rm");
        for m in mounts {
            let m = m.display().to_string();
            cmd.arg("-v").arg(format!("{m}:{m}:ro"));
        }
        cmd.arg(image).args(["sh", "-c", "sleep infinity"]);
        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError::Msg(format!("Failed to turn {runtime}: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolError::Msg(format!(
                "Failed to create container: {stderr}"
            )));
        }

        let id = String::from_utf8(output.stdout)
            .map_err(|e| ToolError::Msg(format!("Non-UTF-8 container ID: {e}")))?
            .trim()
            .to_string();
        if id.is_empty() {
            return Err(ToolError::Msg("Empty container ID".into()));
        }

        Command::new(runtime)
            .args(["start", &id])
            .output()
            .await
            .map_err(|e| ToolError::Msg(format!("Failed to start container: {e}")))?;

        Ok(Self {
            id,
            runtime: runtime.to_string(),
        })
    }
}

impl Drop for ContainerGuard {
    fn drop(&mut self) {
        let _ = std::process::Command::new(&self.runtime)
            .args(["rm", "-f", &self.id])
            .output();
    }
}

#[derive(Debug)]
struct SandboxRuntime {
    runtime: ContainerRuntime,
    image: String,
    mounts: Vec<PathBuf>,
    container: Option<ContainerGuard>,
}

impl SandboxRuntime {
    async fn ensure_container(&mut self) -> Result<&ContainerGuard, ToolError> {
        if self.container.is_none() {
            self.container = Some(
                ContainerGuard::create(self.runtime.as_str(), &self.image, &self.mounts).await?,
            );
        }
        Ok(self.container.as_ref().expect("container just set"))
    }
}

#[derive(Debug)]
pub struct ShellRuntime {
    default_timeout: u64,
    sandbox: Option<SandboxRuntime>,
}

impl ShellRuntime {
    pub fn new(cfg: &SandboxConfig, mounts: &[PathBuf]) -> Self {
        Self {
            default_timeout: cfg.timeout_secs,
            sandbox: cfg.enabled.then(|| SandboxRuntime {
                runtime: cfg.runtime,
                image: cfg.image.clone(),
                mounts: mounts.to_vec(),
                container: None,
            }),
        }
    }

    pub async fn execute(
        &mut self,
        command: &str,
        workdir: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<ShellOutput, ToolError> {
        let timeout = timeout_secs.unwrap_or(self.default_timeout);

        let Some(sb) = &mut self.sandbox else {
            return run_on_host(command, workdir, timeout).await;
        };

        let handle = sb.ensure_container().await?;
        let dir = workdir.unwrap_or_else(|| "/tmp".to_string());
        exec_in_container(handle, command, &dir, timeout).await
    }
}

async fn exec_in_container(
    guard: &ContainerGuard,
    command: &str,
    workdir: &str,
    timeout_secs: u64,
) -> Result<ShellOutput, ToolError> {
    let mut cmd = Command::new(&guard.runtime);
    cmd.args(["exec", "-w", workdir, &guard.id, "bash", "-c", command]);
    run_cmd(cmd, timeout_secs).await
}

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

async fn run_on_host(
    command: &str,
    workdir: Option<String>,
    timeout_secs: u64,
) -> Result<ShellOutput, ToolError> {
    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command);
    if let Some(dir) = &workdir {
        cmd.current_dir(dir);
    }
    run_cmd(cmd, timeout_secs).await
}

async fn run_cmd(mut cmd: Command, timeout_secs: u64) -> Result<ShellOutput, ToolError> {
    let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| ToolError::Msg(format!("Command timed out after {timeout_secs}s")))?;

    let output = result.map_err(|e| ToolError::Msg(format!("Failed to execute command: {e}")))?;

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
