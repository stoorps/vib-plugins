use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::ffi::CString;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::os::raw::c_char;
use std::path::Path;
use vib_api::{build_module, plugin_info, Recipe};

#[derive(Default, Clone, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum As {
    #[default]
    system,
    user,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[plugin_info(name = "boot-shell", module_type = "0", use_container_cmds = "0")]
struct PkgModule {
    name: String,
    r#type: String,

    #[serde(default)]
    packages: Vec<String>,

    #[serde(default)]
    remotes: Vec<String>,
    #[serde(default)]
    r#as: As,

    #[serde(default)]
    commands: Vec<String>,
}


#[build_module]
fn build(module: PkgModule, recipe: Recipe) -> String {
    let includes_dir = Path::new(&recipe.includes_path);
    let service_parent_dir = includes_dir.join("etc/systemd/");
    let script_dir = includes_dir.join("usr/bin/");

    let uuid = Uuid::new_v4().to_string();

    let (script_path, service_dir, service_path, service_cmd) = match module.r#as {
        As::system => (
            script_dir.join(format!("boot-shell-system-{uuid}")),
            service_parent_dir.join("system"),
            service_parent_dir.join(format!("system/boot-shell-system-{uuid}.service")),
            format!("--system boot-shell-system-{uuid}"),
        ),
        As::user => (
            script_dir.join(format!("boot-shell-user-{uuid}")),
            service_parent_dir.join("user"),
            service_parent_dir.join(format!("user/boot-shell-user-{uuid}.service")),
            format!("--user boot-shell-user-{uuid}"),
        ),
    };

    println!("{}\n{}\n{}",includes_dir.display(),script_path.display(),service_path.display());


    let mut script = "
    
    #!/bin/bash
    -oeu pipefail

    ".to_owned();


    for cmd in module.commands
    {
        script.push_str(&format!("{cmd}\n"));
    }



    let script_file = match script_path.exists() {
        true => OpenOptions::new().append(true).open(&script_path),
        false => {
            let script_dir = script_dir.as_path();
            if !script_dir.exists() {
                match create_dir_all(script_dir) {
                    Ok(_) => {}
                    Err(e) => {
                        return format!("Error creating {}: {e}", script_dir.display());
                    }
                }
            }

            OpenOptions::new()
                .write(true)
                .create(true)
                .open(&script_path)
        }
    };

    match script_file {
        Ok(mut script_file) => {
            if let Err(e) = writeln!(script_file, "{script}") {
                return format!("Couldn't write to file: {e}");
            }

            if !service_dir.exists() {
                match create_dir_all(service_dir.clone()) {
                    Ok(_) => {}
                    Err(e) => {
                        return format!("Error creating {}: {e}", service_dir.display());
                    }
                }
            }

            if service_path.exists()
            {
                //Already created and enabled
                return "echo \"service already created\"".into()
            }

            let mut service_file = match OpenOptions::new()
                .write(true)
                .create(true)
                .open(service_path.clone())
            {
                Ok(service_file) => service_file,
                Err(e) => return format!("Error creating {}: {e}", service_path.display()),
            };

            let service_definition = match module.r#as {
                As::system => format!(
                    "
[Unit]
Description=Runs scripts after boot
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
ExecStart={}
Restart=on-failure
RestartSec=30

[Install]
WantedBy=default.target",
                    script_path.display()
                ),
                As::user => format!(
                    "
[Unit]
Description=Runs scripts after boot
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
ExecStart={}
Restart=on-failure
RestartSec=30

[Install]
WantedBy=default.target",
                    script_path.display()
                ),
            };

            if let Err(e) = writeln!(service_file, "{service_definition}") {
                return format!("Couldn't write to file: {e}");
            }

            return format!("systemctl enable {service_cmd}");
        }
        Err(e) => {
            return format!("Error setting up boot module: {e}");
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_build_system_module() {
        let temp_dir = tempdir().unwrap();
        let includes_path = temp_dir.path().join("includes");
        fs::create_dir_all(&includes_path).unwrap();

        let module = PkgModule {
            name: "test-module".to_string(),
            r#type: "boot-shell".to_string(),
            commands: vec!["echo 'hello'".to_string(), "ls -l".to_string()],
            r#as: As::system,
            ..Default::default()
        };

        let recipe = Recipe {
            includes_path: includes_path.to_str().unwrap().to_string(),
            ..Default::default()
        };

        let result = build(module, recipe);

        assert!(result.starts_with("systemctl enable --system boot-shell-system-"));
        let uuid = result.split("boot-shell-system-").last().unwrap();
        let uuid = uuid.trim();

        let script_path = temp_dir.path().join(format!("includes/usr/bin/boot-shell-system-{uuid}"));
        let service_path = temp_dir.path().join(format!("includes/etc/systemd/system/boot-shell-system-{uuid}.service"));

        assert!(script_path.exists());
        assert!(service_path.exists());

        let script_content = fs::read_to_string(&script_path).unwrap();
        assert!(script_content.contains("echo 'hello'"));
        assert!(script_content.contains("ls -l"));

        let service_content = fs::read_to_string(service_path).unwrap();
        assert!(service_content.contains(script_path.to_str().unwrap()));
    }
}