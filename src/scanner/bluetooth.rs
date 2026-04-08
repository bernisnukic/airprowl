use std::sync::Arc;
use bluer::{Adapter, AdapterEvent, DeviceEvent, DeviceProperty};
use futures::{pin_mut, StreamExt, stream::SelectAll};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::events::{AppEvent, BtUpdate};

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
            Some(event) = device_events.next() => {
                match event {
                    AdapterEvent::DeviceAdded(addr) => {
                        let device = adapter.device(addr)?;
                        let update = BtUpdate {
                            address: addr.to_string(),
                            name: device.name().await.ok().flatten()
                                .or(device.alias().await.ok()),
                            rssi: device.rssi().await.ok().flatten(),
                            tx_power: device.tx_power().await.ok().flatten(),
                        };
                        tx.send(AppEvent::BtUpdate(update)).await.ok();

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
                        BtUpdate { address: addr.to_string(), name: Some(v), ..Default::default() }
                    }
                    DeviceEvent::PropertyChanged(DeviceProperty::Alias(v)) => {
                        BtUpdate { address: addr.to_string(), name: Some(v), ..Default::default() }
                    }
                    _ => continue,
                };
                tx.send(AppEvent::BtUpdate(update)).await.ok();
            }

            else => break,
        }
    }

    Ok(())
}
