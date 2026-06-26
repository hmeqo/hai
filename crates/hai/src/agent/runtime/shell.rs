use std::path::Path;

use autoagents::core::tool::ToolCallError;
use tokio::process::Command;

use crate::config::schema::{ContainerRuntime, SandboxConfig};

#[derive(Debug)]
pub struct ContainerGuard {
    runtime: String,
    id: String,
}

impl ContainerGuard {
    pub async fn create(runtime: &str, image: &str) -> Result<Self, ToolCallError> {
        let output = Command::new(runtime)
            .args(["create", "--rm", image, "sleep", "infinity"])
            .output()
            .await
            .map_err(|e| {
                ToolCallError::RuntimeError(format!("Failed to run {runtime}: {e}").into())
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ToolCallError::RuntimeError(
                format!("Failed to create container: {stderr}").into(),
            ));
        }

        let id = String::from_utf8(output.stdout)
            .map_err(|e| {
                ToolCallError::RuntimeError(format!("Non-UTF-8 container ID: {e}").into())
            })?
            .trim()
            .to_string();
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

        Ok(Self {
            id,
            runtime: runtime.to_string(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn runtime(&self) -> &str {
        &self.runtime
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
    container: Option<ContainerGuard>,
}

impl SandboxRuntime {
    async fn ensure_container(&mut self) -> Result<&ContainerGuard, ToolCallError> {
        if self.container.is_none() {
            self.container =
                Some(ContainerGuard::create(self.runtime.as_str(), &self.image).await?);
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

        let handle = sb.ensure_container().await?;
        if let Some(dir) = skill_dir {
            copy_to_container(handle.runtime(), handle.id(), dir, "/workspace").await?;
        }
        exec_in_container(handle.runtime(), handle.id(), command, timeout).await
    }
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
        return Err(ToolCallError::RuntimeError(
            format!("docker cp failed: {stderr}").into(),
        ));
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
