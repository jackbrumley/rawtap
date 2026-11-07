//! Before starting ssh: check the server answers, so RawTap only says it
//! reached a server it really reached. ssh does the real connecting inside
//! its pseudo-terminal and prints nothing while it waits, so without this a
//! host that's off or behind a down VPN just looks like a blank screen.
//!
//! `ssh -G` gives the address and port ssh will really use (after
//! ~/.ssh/config: HostName, Port), and whether it goes through a jump host
//! (ProxyJump / ProxyCommand), which only ssh can reach: then there's no
//! check.

use std::process::{Command, Stdio};

/// What `ssh -G` says ssh will connect to.
#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub hostname: String,
    pub port: u16,
    /// The jump host or proxy command, when ssh goes through one.
    pub via: Option<String>,
}

/// The arguments RawTap gives ssh for a destination.
pub fn args(host: &str, user: &str, port: u16) -> Vec<String> {
    let mut args = vec!["-p".to_string(), port.to_string()];
    if !user.is_empty() {
        args.extend(["-l".to_string(), user.to_string()]);
    }
    args.push(host.to_string());
    args
}

/// Asks `ssh -G` (local, no network) where ssh will connect.
pub fn target(host: &str, user: &str, port: u16) -> Result<Target, String> {
    let output = Command::new("ssh")
        .arg("-G")
        .args(args(host, user, port))
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Could not run ssh: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh couldn't read its settings for {host}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_target(&String::from_utf8_lossy(&output.stdout), host, port)
}

fn parse_target(config: &str, host: &str, port: u16) -> Result<Target, String> {
    let mut target = Target {
        hostname: host.to_string(),
        port,
        via: None,
    };
    for line in config.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let value = value.trim();
        match key {
            "hostname" => target.hostname = value.to_string(),
            "port" => {
                target.port = value
                    .parse()
                    .map_err(|_| format!("ssh reported an invalid port '{value}' for {host}"))?
            }
            "proxyjump" | "proxycommand" if value != "none" => {
                target.via = Some(format!("{key} {value}"))
            }
            _ => {}
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_leave_out_an_empty_user() {
        assert_eq!(args("box", "", 22), ["-p", "22", "box"]);
        assert_eq!(
            args("box", "admin", 2222),
            ["-p", "2222", "-l", "admin", "box"]
        );
    }

    #[test]
    fn config_gives_the_real_address_and_port() {
        let config = "user admin\nhostname 10.0.0.5\nport 2200\naddressfamily any\n";
        assert_eq!(
            parse_target(config, "lab", 22).unwrap(),
            Target {
                hostname: "10.0.0.5".into(),
                port: 2200,
                via: None
            }
        );
    }

    #[test]
    fn a_jump_host_is_noted_and_none_is_ignored() {
        let config = "hostname 10.0.0.5\nport 22\nproxyjump bastion\nproxycommand none\n";
        assert_eq!(
            parse_target(config, "lab", 22).unwrap().via.as_deref(),
            Some("proxyjump bastion")
        );
    }
}
