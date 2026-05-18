use std::sync::Arc;
use bluer::{Adapter, AddressType, AdapterEvent, DeviceEvent, DeviceProperty};
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
                        let update = BtUpdate {
                            address: addr_str.clone(),
                            name: raw_name.and_then(|n| clean_bt_name(n, &addr_str)),
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
