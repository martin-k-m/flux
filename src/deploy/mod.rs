//! Flux Deployment System (2.7).
//!
//! `flux deploy` reads the `.flux` `deployment { … }` block and dispatches to a
//! target handler: local, docker, kubernetes, or a cloud VM. Each handler does
//! real work **when the required tool is present**, and degrades honestly when
//! it isn't (it prints exactly what it would run, and returns a non-success
//! code) rather than pretending to have deployed.

use std::path::Path;
use std::process::Command;

use crate::core::config::Deployment;
use crate::core::logging as log;
use crate::runners::containers;

/// The outcome of a deploy attempt.
pub struct DeployResult {
    pub success: bool,
    /// True when we couldn't act because a required tool was missing.
    pub tool_missing: bool,
}

/// Deploy `project` according to `deployment`.
pub fn deploy(root: &Path, project: &str, deployment: &Deployment) -> DeployResult {
    let target = deployment.target.as_deref().unwrap_or("local");
    log::field("Target", target);
    if let Some(r) = deployment.replicas {
        log::field("Replicas", &r.to_string());
    }
    if let Some(img) = &deployment.image {
        log::field("Image", img);
    }
    println!();

    match target {
        "local" => deploy_local(project, deployment),
        "docker" => deploy_docker(project, deployment),
        "kubernetes" | "k8s" => deploy_kubernetes(root, project, deployment),
        "vm" | "cloud" => deploy_vm(target),
        other => {
            log::fail_line(&format!("unknown deploy target '{other}'"));
            DeployResult {
                success: false,
                tool_missing: false,
            }
        }
    }
}

fn deploy_local(project: &str, deployment: &Deployment) -> DeployResult {
    // "Local machine" deploy: if an image is given and an engine exists, run it;
    // otherwise there is nothing to orchestrate locally beyond the build output.
    if let Some(image) = &deployment.image {
        if let Some(engine) = containers::engine() {
            let cmd = format!("{engine} run --rm -d --name {project} {image}");
            return run_and_report(&cmd, engine);
        }
        log::fail_line("no container engine found to run the image locally");
        return DeployResult {
            success: false,
            tool_missing: true,
        };
    }
    log::ok_line("Local deploy: build outputs are ready to run in place");
    DeployResult {
        success: true,
        tool_missing: false,
    }
}

fn deploy_docker(project: &str, deployment: &Deployment) -> DeployResult {
    let image = match &deployment.image {
        Some(i) => i.clone(),
        None => format!("{project}:latest"),
    };
    match containers::engine() {
        Some(engine) => {
            let cmd = format!("{engine} run --rm -d --name {project} {image}");
            run_and_report(&cmd, engine)
        }
        None => {
            log::fail_line("no container engine (docker/podman) found");
            log::info_line(&format!(
                "  {} would run: docker run --rm -d --name {project} {image}",
                log::dim(log::DOT)
            ));
            DeployResult {
                success: false,
                tool_missing: true,
            }
        }
    }
}

fn deploy_kubernetes(root: &Path, project: &str, deployment: &Deployment) -> DeployResult {
    let manifest = k8s_manifest(project, deployment);
    let dir = root.join(".flux-cache").join("deploy");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{project}-deployment.yaml"));
    if std::fs::write(&path, &manifest).is_ok() {
        log::ok_line(&format!("Wrote manifest: {}", path.display()));
    }

    if tool_available("kubectl") {
        let cmd = format!("kubectl apply -f \"{}\"", path.display());
        run_and_report(&cmd, "kubectl")
    } else {
        log::fail_line("kubectl not found — manifest written but not applied");
        log::info_line(&format!(
            "\n{}\n{}",
            log::dim("--- generated manifest ---"),
            manifest
        ));
        DeployResult {
            success: false,
            tool_missing: true,
        }
    }
}

fn deploy_vm(target: &str) -> DeployResult {
    log::fail_line(&format!("'{target}' deploy target is not supported"));
    log::info_line(&format!(
        "  {} cloud-VM deploys (SSH/rsync/systemd) are out of scope — use the local, docker, or kubernetes targets",
        log::dim(log::DOT)
    ));
    DeployResult {
        success: false,
        tool_missing: true,
    }
}

/// Generate a minimal but valid Kubernetes Deployment manifest.
fn k8s_manifest(project: &str, deployment: &Deployment) -> String {
    let replicas = deployment.replicas.unwrap_or(1);
    let image = deployment
        .image
        .clone()
        .unwrap_or_else(|| format!("{project}:latest"));
    format!(
        "apiVersion: apps/v1\n\
         kind: Deployment\n\
         metadata:\n\
         \x20 name: {project}\n\
         \x20 labels:\n\
         \x20   app: {project}\n\
         spec:\n\
         \x20 replicas: {replicas}\n\
         \x20 selector:\n\
         \x20   matchLabels:\n\
         \x20     app: {project}\n\
         \x20 template:\n\
         \x20   metadata:\n\
         \x20     labels:\n\
         \x20       app: {project}\n\
         \x20   spec:\n\
         \x20     containers:\n\
         \x20       - name: {project}\n\
         \x20         image: {image}\n"
    )
}

fn run_and_report(cmd: &str, tool: &str) -> DeployResult {
    log::info_line(&format!("  {} {}", log::cyan(log::ARROW), log::dim(cmd)));
    match crate::runners::shell::run(cmd, Path::new(".")) {
        Ok(res) if res.success => {
            log::ok_line(&format!("{tool} deploy succeeded"));
            DeployResult {
                success: true,
                tool_missing: false,
            }
        }
        Ok(_) => {
            log::fail_line(&format!("{tool} deploy failed"));
            DeployResult {
                success: false,
                tool_missing: false,
            }
        }
        Err(e) => {
            log::fail_line(&format!("could not run {tool}: {e}"));
            DeployResult {
                success: false,
                tool_missing: true,
            }
        }
    }
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k8s_manifest_has_replicas_and_image() {
        let dep = Deployment {
            target: Some("kubernetes".into()),
            replicas: Some(3),
            image: Some("myapp:1.2".into()),
        };
        let m = k8s_manifest("myapp", &dep);
        assert!(m.contains("kind: Deployment"));
        assert!(m.contains("replicas: 3"));
        assert!(m.contains("image: myapp:1.2"));
        assert!(m.contains("app: myapp"));
    }
}
