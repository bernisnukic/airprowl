use std::sync::Arc;
use tokio::process::Command;

use crate::config::Config;

pub struct MonitorHandle {
    pub sniff_iface: String,
    config: Arc<Config>,
}

impl MonitorHandle {
    async fn cmd(args: &[&str]) -> bool {
        Command::new(args[0])
            .args(&args[1..])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        let mon_iface = self.config.mon_iface.clone();
        let is_root = self.config.is_root;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let sudo: Vec<&str> = if is_root { vec![] } else { vec!["sudo", "-n"] };
                let mut down: Vec<&str> = sudo.clone();
                down.extend(["ip", "link", "set", &mon_iface, "down"]);
                MonitorHandle::cmd(&down).await;
                let mut del: Vec<&str> = sudo;
                del.extend(["iw", "dev", &mon_iface, "del"]);
                MonitorHandle::cmd(&del).await;
            });
        });
    }
}

pub async fn setup_monitor(config: Arc<Config>) -> anyhow::Result<MonitorHandle> {
    let sudo = if config.is_root { vec![] } else { vec!["sudo", "-n"] };
    let iface = &config.wifi_iface;
    let mon_iface = &config.mon_iface;

    // Check if monitor interface already exists
    let output = Command::new("iw").arg("dev").output().await?;
    let iw_out = String::from_utf8_lossy(&output.stdout);
    if iw_out.contains(mon_iface.as_str()) {
        return Ok(MonitorHandle {
            sniff_iface: mon_iface.clone(),
            config,
        });
    }

    // Create virtual monitor interface (keeps managed iface up)
    let mut add_args: Vec<&str> = sudo.clone();
    add_args.extend(["iw", "dev", iface, "interface", "add", mon_iface, "type", "monitor"]);
    if MonitorHandle::cmd(&add_args).await {
        let mut up_args: Vec<&str> = sudo;
        up_args.extend(["ip", "link", "set", mon_iface, "up"]);
        MonitorHandle::cmd(&up_args).await;
        return Ok(MonitorHandle {
            sniff_iface: mon_iface.clone(),
            config,
        });
    }

    anyhow::bail!("Could not create monitor interface. Run with sudo for client sniffing.")
}
