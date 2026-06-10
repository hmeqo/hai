use std::path::Path;

use autoagents::core::tool::ToolCallError;
use tokio::process::Command;

use crate::config::schema::{ContainerRuntime, SandboxConfig};

pub struct ContainerHandle {
    pub id: String,
    pub runtime: String,
}

impl Drop for ContainerHandle {
    fn drop(&mut self) {
        let id = self.id.clone();
        let runtime = self.runtime.clone();
        tokio::spawn(async move {
            let output = Command::new(&runtime)
                .args(["rm", "-f", &id])
                .output()
                .await;
            if let Err(e) = output {
                tracing::warn!("Failed to destroy container {}: {e}", id);
            }
        });
    }
}

struct SandboxRuntime {
    runtime: ContainerRuntime,
    image: String,
    container: Option<ContainerHandle>,
}

impl SandboxRuntime {
    async fn ensure_container(&mut self) -> Result<&ContainerHandle, ToolCallError> {
        if self.container.is_none() {
            let handle = create_container(self.runtime.as_str(), &self.image).await?;
            self.container = Some(handle);
        }
        Ok(self.container.as_ref().unwrap())
    }
}

pub struct ShellRuntime {
    default_timeout: u64,
    sandbox: Option<SandboxRuntime>,
}

impl ShellRuntime {
    pub fn new(cfg: &SandboxConfig) -> Self {
        Self {
            default_timeout: cfg.timeout_secs,
            sandbox: cfg.enabled.then(|| SandboxRuntime {
                runtime: cfg.runtime,
                image: cfg.image.clone(),
                container: None,
            }),
        }
    }

    pub fn sandbox_enabled(&self) -> bool {
        self.sandbox.is_some()
    }

    pub async fn execute(
        &mut self,
        command: &str,
        workdir: Option<String>,
        skill_dir: Option<&Path>,
        timeout_secs: Option<u64>,
    ) -> Result<ShellOutput, ToolCallError> {
        let timeout = timeout_secs.unwrap_or(self.default_timeout);

        let Some(sb) = &mut self.sandbox else {
            let dir = skill_dir.map(|p| p.display().to_string()).or(workdir);
            return run_on_host(command, dir, timeout).await;
        };

        let runtime = sb.runtime.as_str();
        let handle = sb.ensure_container().await?;
        if let Some(dir) = skill_dir {
            copy_to_container(runtime, &handle.id, dir, "/workspace").await?;
        }
        exec_in_container(runtime, &handle.id, command, timeout).await
    }
}

async fn create_container(runtime: &str, image: &str) -> Result<ContainerHandle, ToolCallError> {
    let output = Command::new(runtime)
        .args(["create", "--rm", image, "sleep", "infinity"])
        .output()
        .await
        .map_err(|e| ToolCallError::RuntimeError(format!("Failed to run {runtime}: {e}").into()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ToolCallError::RuntimeError(
            format!("Failed to create container: {stderr}").into(),
        ));
    }

    let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if id.is_empty() {
        return Err(ToolCallError::RuntimeError("Empty container ID".into()));
    }

    Command::new(runtime)
        .args(["start", &id])
        .output()
        .await
        .map_err(|e| {
            ToolCallError::RuntimeError(format!("Failed to start container: {e}").into())
        })?;

    Ok(ContainerHandle {
        id,
        runtime: runtime.to_string(),
    })
}

async fn exec_in_container(
    runtime: &str,
    container_id: &str,
    command: &str,
    timeout_secs: u64,
) -> Result<ShellOutput, ToolCallError> {
    let mut cmd = Command::new(runtime);
    cmd.args([
        "exec",
        "-w",
        "/workspace",
        container_id,
        "bash",
        "-c",
        command,
    ]);
    run_cmd(cmd, timeout_secs).await
}

async fn copy_to_container(
    runtime: &str,
    container_id: &str,
    src: &Path,
    dest: &str,
) -> Result<(), ToolCallError> {
    let src_str = src.display().to_string();
    let dest_str = format!("{}:{}", container_id, dest);
    let output = Command::new(runtime)
        .args(["cp", &src_str, &dest_str])
        .output()
        .await
        .map_err(|e| ToolCallError::RuntimeError(format!("docker cp failed: {e}").into()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("docker cp warning: {stderr}");
    }
    Ok(())
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
) -> Result<ShellOutput, ToolCallError> {
    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command);
    if let Some(dir) = &workdir {
        cmd.current_dir(dir);
    }
    run_cmd(cmd, timeout_secs).await
}

async fn run_cmd(mut cmd: Command, timeout_secs: u64) -> Result<ShellOutput, ToolCallError> {
    let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), cmd.output())
        .await
        .map_err(|_| {
            ToolCallError::RuntimeError(format!("Command timed out after {timeout_secs}s").into())
        })?;

    let output = result.map_err(|e| {
        ToolCallError::RuntimeError(format!("Failed to execute command: {e}").into())
    })?;

    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
