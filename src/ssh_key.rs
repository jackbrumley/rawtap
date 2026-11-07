//! SSH key login: finding (or making) the user's public key, and the shell
//! command that adds it to a server's `~/.ssh/authorized_keys`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// ssh's default identities, in the order ssh tries them.
const KEY_FILES: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

/// The user's public key, made with ssh-keygen (no passphrase) when there's
/// none. Returns the key and, when it was just made, its file.
pub fn public_key_or_create(comment: &str) -> Result<(String, Option<PathBuf>), String> {
    let dir = crate::paths::ssh_dir()?;
    for name in KEY_FILES {
        let public = dir.join(format!("{name}.pub"));
        if dir.join(name).exists() && public.exists() {
            return read_public(&public).map(|key| (key, None));
        }
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let private = dir.join(KEY_FILES[0]);
    let output = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
        .arg(&private)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run ssh-keygen: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh-keygen could not make a key: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    log::info!("Made an SSH key: {}", private.display());
    let public = dir.join(format!("{}.pub", KEY_FILES[0]));
    read_public(&public).map(|key| (key, Some(private)))
}

fn read_public(path: &std::path::Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|k| k.trim().to_string())
        .map_err(|e| format!("Could not read {}: {e}", path.display()))
}

/// The command typed into the server's shell: adds the key once, with the
/// permissions sshd insists on, and says when it's done.
pub fn authorize_command(key: &str) -> Result<String, String> {
    let looks_like_a_key = key.starts_with("ssh-") || key.starts_with("ecdsa-");
    if !looks_like_a_key || key.contains(['\'', '\n', '\r']) {
        return Err("Your public key file doesn't look like an SSH public key.".into());
    }
    Ok(format!(
        "umask 077; mkdir -p ~/.ssh && {{ grep -qxF '{key}' ~/.ssh/authorized_keys 2>/dev/null || echo '{key}' >> ~/.ssh/authorized_keys; }} && echo 'RawTap: key login is set up.'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_adds_the_key_once_and_refuses_odd_keys() {
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample RawTap for admin@lab";
        let command = authorize_command(key).unwrap();
        assert!(command.contains(&format!("grep -qxF '{key}'")));
        assert!(command.contains(&format!("echo '{key}' >> ~/.ssh/authorized_keys")));
        assert!(authorize_command("ssh-ed25519 AAAA it's").is_err());
        assert!(authorize_command("not a key").is_err());
    }
}
