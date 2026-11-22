use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use bluer::{Adapter, Address, AddressType, AdapterEvent, DeviceEvent, DeviceProperty, Uuid};
use futures::{pin_mut, StreamExt, stream::SelectAll};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::events::{AppEvent, BtUpdate};

/// Treat empty strings and MAC-shaped fallback aliases as "no name", so the
/// widget's OUI-vendor fallback kicks in. BlueZ defaults `alias` to the MAC
/// when no advertised name exists; without this filter the address looks like
/// a real name to the rest of the app.
fn clean_bt_name(name: String, addr: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let hex_only = |s: &str| -> String {
        s.chars().filter(|c| c.is_ascii_hexdigit()).map(|c| c.to_ascii_uppercase()).collect()
    };
    if hex_only(&name) == hex_only(addr) {
        return None;
    }
    Some(name)
}

/// ASTM F3411 Remote ID service UUID 0xFFFA.
const REMOTE_ID_UUID: Uuid = Uuid::from_u128(0x0000fffa_0000_1000_8000_00805f9b34fb);

/// Decode Remote ID service data into a row label: the trust verdict first,
/// so it survives column truncation, then the drone's serial.
fn drone_label(
    tracker: &mut remoteid::Tracker,
    addr: Address,
    service_data: &HashMap<Uuid, Vec<u8>>,
    now_ms: u64,
) -> Option<String> {
    let rid = remoteid::ble::parse_service_data(service_data.get(&REMOTE_ID_UUID)?).ok()?;
    let uas = tracker.ingest_ble(addr.0, &rid, now_ms);
    Some(format!("{} drone {}", uas.assessment.verdict, uas.uas_id()?))
}

pub async fn run_bt_scanner(
    config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    let session = bluer::Session::new().await?;

    let adapter: Adapter = if let Some(ref name) = config.bt_iface {
        session.adapter(name)?
    } else {
        session.default_adapter().await?
    };

    adapter.set_powered(true).await?;

    if let Err(e) = adapter
        .set_discovery_filter(bluer::DiscoveryFilter {
            transport: bluer::DiscoveryTransport::Auto,
            ..Default::default()
        })
        .await
    {
        // Non-fatal: filter might already be set
        eprintln!("Discovery filter warning: {}", e);
    }

    let device_events = adapter.discover_devices().await?;
    pin_mut!(device_events);

    let mut change_streams = SelectAll::new();
    let mut drones = remoteid::Tracker::new(remoteid::TrackerConfig::default());
    let started = Instant::now();

    loop {
        tokio::select! {
            biased;
            _ = tx.closed() => break,

            Some(event) = device_events.next() => {
                match event {
                    AdapterEvent::DeviceAdded(addr) => {
                        let device = adapter.device(addr)?;
                        let addr_str = addr.to_string();
                        let raw_name = device.name().await.ok().flatten()
                            .or(device.alias().await.ok());
                        let is_random = matches!(
                            device.address_type().await,
                            Ok(AddressType::LeRandom)
                        );
                        let icon = device.icon().await.ok().flatten()
                            .filter(|s| !s.is_empty());
                        let company_id = device.manufacturer_data().await.ok().flatten()
                            .and_then(|m| m.keys().min().copied());
                        let drone = device.service_data().await.ok().flatten().and_then(|sd| {
                            drone_label(&mut drones, addr, &sd, started.elapsed().as_millis() as u64)
                        });
                        let update = BtUpdate {
                            address: addr_str.clone(),
                            name: drone.or(raw_name.and_then(|n| clean_bt_name(n, &addr_str))),
                            rssi: device.rssi().await.ok().flatten(),
                            tx_power: device.tx_power().await.ok().flatten(),
                            is_random,
                            icon,
                            company_id,
                        };
                        if tx.send(AppEvent::BtUpdate(update)).await.is_err() {
                            return Ok(());
                        }

                        if let Ok(events) = device.events().await {
                            let addr_clone = addr;
                            let mapped = events.map(move |e| (addr_clone, e));
                            change_streams.push(mapped);
                        }
                    }
                    AdapterEvent::DeviceRemoved(_) => {}
                    _ => {}
                }
            }

            Some((addr, event)) = change_streams.next() => {
                let update = match event {
                    DeviceEvent::PropertyChanged(DeviceProperty::Rssi(v)) => {
                        BtUpdate { address: addr.to_string(), rssi: Some(v), ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::TxPower(v)) => {
                        BtUpdate { address: addr.to_string(), tx_power: Some(v), ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::Name(v)) => {
                        let addr_str = addr.to_string();
                        let name = clean_bt_name(v, &addr_str);
                        if name.is_none() { continue; }
                        BtUpdate { address: addr_str, name, ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::Alias(v)) => {
                        let addr_str = addr.to_string();
                        let name = clean_bt_name(v, &addr_str);
                        if name.is_none() { continue; }
                        BtUpdate { address: addr_str, name, ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::Icon(v)) => {
                        if v.is_empty() { continue; }
                        BtUpdate { address: addr.to_string(), icon: Some(v), ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::ManufacturerData(m)) => {
                        let Some(cid) = m.keys().min().copied() else { continue };
                        BtUpdate { address: addr.to_string(), company_id: Some(cid), ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::ServiceData(sd)) => {
                        let now_ms = started.elapsed().as_millis() as u64;
                        let Some(name) = drone_label(&mut drones, addr, &sd, now_ms) else { continue };
                        BtUpdate { address: addr.to_string(), name: Some(name), ..Default::default() }
                    }
                    _ => continue,
                };
                if tx.send(AppEvent::BtUpdate(update)).await.is_err() {
                    return Ok(());
                }
            }

            else => break,
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use remoteid::{BasicId, IdType, Message, UaType};

    fn service_data(serial: &str) -> HashMap<Uuid, Vec<u8>> {
        let msg = Message::BasicId(BasicId::new(IdType::SerialNumber, UaType::HelicopterOrMultirotor, serial).unwrap());
        let mut data = vec![remoteid::ble::APP_CODE, 0];
        data.extend_from_slice(&msg.encode().unwrap());
        HashMap::from([(REMOTE_ID_UUID, data)])
    }

    #[test]
    fn remote_id_advert_becomes_a_drone_row() {
        let mut tracker = remoteid::Tracker::new(remoteid::TrackerConfig::default());
        let addr = Address::new([0x60, 0x60, 0x1f, 0x01, 0x02, 0x03]);
        let label = drone_label(&mut tracker, addr, &service_data("1581F5FJD239C00DQ5RH"), 0);
        assert_eq!(label.as_deref(), Some("plausible drone 1581F5FJD239C00DQ5RH"));
    }

    #[test]
    fn other_service_data_is_not_a_drone() {
        let mut tracker = remoteid::Tracker::new(remoteid::TrackerConfig::default());
        let addr = Address::new([0x60, 0x60, 0x1f, 0x01, 0x02, 0x03]);
        let sd = HashMap::from([(Uuid::from_u128(0x0000180f_0000_1000_8000_00805f9b34fb), vec![100])]);
        assert_eq!(drone_label(&mut tracker, addr, &sd, 0), None);
    }
}
