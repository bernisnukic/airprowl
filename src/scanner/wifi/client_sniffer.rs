use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use pnet::datalink::{self, Channel, Config};
use libwifi::Frame;

use crate::events::{AppEvent, WifiClientUpdate};

pub async fn run_client_sniffer(
    mon_iface: String,
    tx: mpsc::Sender<AppEvent>,
    known_aps: Arc<Mutex<HashSet<String>>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let interfaces = datalink::interfaces();
    let iface = interfaces
        .into_iter()
        .find(|i| i.name == mon_iface)
        .ok_or_else(|| anyhow::anyhow!("Interface {} not found", mon_iface))?;

    let config = Config {
        promiscuous: true,
        ..Default::default()
    };

    let (_tx_chan, mut rx) = match datalink::channel(&iface, config)? {
        Channel::Ethernet(tx, rx) => (tx, rx),
        _ => anyhow::bail!("Unexpected channel type for {}", mon_iface),
    };

    let tx_event = tx.clone();

    let handle = tokio::task::spawn_blocking(move || {
        let tx = tx_event;
        loop {
            if cancel.is_cancelled() {
                break;
            }

            let packet = match rx.next() {
                Ok(p) => p,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
            };

            if packet.len() < 4 {
                continue;
            }
            let rt_len = u16::from_le_bytes([packet[2], packet[3]]) as usize;
            if packet.len() <= rt_len {
                continue;
            }

            let rssi = extract_rssi(packet, rt_len);
            let frame_data = &packet[rt_len..];

            let frame = match libwifi::parse_frame(frame_data, false) {
                Ok(f) => f,
                Err(_) => continue,
            };

            match frame {
                Frame::ProbeRequest(req) => {
                    let src = req.header.address_2;
                    if src.is_mcast() {
                        continue;
                    }
                    let ssid = req.station_info.ssid.filter(|s| !s.is_empty());
                    tx.blocking_send(AppEvent::WifiClientUpdate(WifiClientUpdate {
                        mac: src.to_string().to_uppercase(),
                        rssi,
                        probing_for: ssid,
                    })).ok();
                }
                Frame::ProbeResponse(resp) => {
                    let src = resp.header.address_2;
                    known_aps.blocking_lock().insert(src.to_string().to_lowercase());
                }
                Frame::Data(data) => {
                    let src = data.header.address_2;
                    if src.is_mcast() { continue; }
                    if known_aps.blocking_lock().contains(&src.to_string().to_lowercase()) { continue; }
                    tx.blocking_send(AppEvent::WifiClientUpdate(WifiClientUpdate {
                        mac: src.to_string().to_uppercase(),
                        rssi,
                        probing_for: None,
                    })).ok();
                }
                Frame::QosData(qos) => {
                    let src = qos.header.address_2;
                    if src.is_mcast() { continue; }
                    if known_aps.blocking_lock().contains(&src.to_string().to_lowercase()) { continue; }
                    tx.blocking_send(AppEvent::WifiClientUpdate(WifiClientUpdate {
                        mac: src.to_string().to_uppercase(),
                        rssi,
                        probing_for: None,
                    })).ok();
                }
                _ => {}
            }
        }
    });

    handle.await?;
    Ok(())
}

fn extract_rssi(data: &[u8], rt_len: usize) -> Option<i16> {
    if data.len() < 8 || rt_len < 8 {
        return None;
    }
    let present = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let mut offset = 8usize;

    let mut p = present;
    while p & (1 << 31) != 0 {
        offset += 4;
        if offset + 4 > data.len() { return None; }
        p = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
    }

    if present & (1 << 0) != 0 { offset = (offset + 7) & !7; offset += 8; }
    if present & (1 << 1) != 0 { offset += 1; }
    if present & (1 << 2) != 0 { offset += 1; }
    if present & (1 << 3) != 0 { offset = (offset + 1) & !1; offset += 4; }
    if present & (1 << 4) != 0 { offset += 2; }
    if present & (1 << 5) != 0 {
        if offset < rt_len && offset < data.len() {
            return Some(data[offset] as i8 as i16);
        }
    }
    None
}
