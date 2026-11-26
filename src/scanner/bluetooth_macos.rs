//! Bluetooth LE scanning on macOS through CoreBluetooth (via btleplug).
//!
//! Differences from the BlueZ scanner that the rest of the app sees:
//! - macOS never reveals a device's MAC address. Each device gets a UUID that
//!   is stable on this Mac only, and that UUID is used as the device key, so
//!   OUI vendor lookup does not apply (manufacturer company IDs still do).
//! - Only BLE advertisers are seen; classic BR/EDR inquiry is not exposed.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow};
use btleplug::api::{Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::events::{AppEvent, BtUpdate};

/// How long to wait for the radio to report it is on before giving up.
const POWER_ON_WAIT: Duration = Duration::from_secs(5);

pub async fn run_bt_scanner(
    _config: Arc<Config>,
    tx: mpsc::Sender<AppEvent>,
) -> anyhow::Result<()> {
    let manager = Manager::new().await.map_err(explain)?;
    let central = manager
        .adapters()
        .await
        .map_err(explain)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no Bluetooth adapter found"))?;

    let mut events = central.events().await.map_err(explain)?;
    wait_until_powered_on(&central, &mut events).await?;
    central
        .start_scan(ScanFilter::default())
        .await
        .map_err(explain)
        .context("could not start scanning")?;

    loop {
        tokio::select! {
            biased;
            _ = tx.closed() => break,
            event = events.next() => {
                let Some(event) = event else { break };
                let update = match event {
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                        match full_update(&central, &id).await {
                            Some(u) => u,
                            None => continue,
                        }
                    }
                    CentralEvent::RssiUpdate { id, rssi } => BtUpdate {
                        address: id.to_string(),
                        rssi: Some(rssi),
                        ..Default::default()
                    },
                    CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } => {
                        let Some(cid) = manufacturer_data.keys().min().copied() else { continue };
                        BtUpdate { address: id.to_string(), company_id: Some(cid), ..Default::default() }
                    }
                    CentralEvent::StateUpdate(CentralState::PoweredOff) => {
                        return Err(anyhow!("Bluetooth was turned off"));
                    }
                    _ => continue,
                };
                if tx.send(AppEvent::BtUpdate(update)).await.is_err() {
                    break;
                }
            }
        }
    }

    let _ = central.stop_scan().await;
    Ok(())
}

/// Everything CoreBluetooth currently knows about one device.
async fn full_update(central: &Adapter, id: &PeripheralId) -> Option<BtUpdate> {
    let props = central.peripheral(id).await.ok()?.properties().await.ok()??;
    let name = props
        .local_name
        .or(props.advertisement_name)
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    Some(BtUpdate {
        address: id.to_string(),
        name,
        rssi: props.rssi,
        tx_power: props.tx_power_level,
        is_random: false,
        icon: None,
        company_id: props.manufacturer_data.keys().min().copied(),
    })
}

/// CoreBluetooth starts in an unknown state and reports the real one shortly
/// after. Scanning before it is powered on silently finds nothing.
async fn wait_until_powered_on(
    central: &Adapter,
    events: &mut (impl futures::Stream<Item = CentralEvent> + Unpin),
) -> anyhow::Result<()> {
    let check = async {
        loop {
            match central.adapter_state().await {
                Ok(CentralState::PoweredOn) => return Ok(()),
                Ok(CentralState::PoweredOff) => {
                    return Err(anyhow!("Bluetooth is off; turn it on in Control Center"));
                }
                _ => {}
            }
            match events.next().await {
                Some(CentralEvent::StateUpdate(CentralState::PoweredOn)) => return Ok(()),
                Some(CentralEvent::StateUpdate(CentralState::PoweredOff)) => {
                    return Err(anyhow!("Bluetooth is off; turn it on in Control Center"));
                }
                Some(_) => {}
                None => return Err(anyhow!("Bluetooth event stream closed")),
            }
        }
    };
    tokio::time::timeout(POWER_ON_WAIT, check)
        .await
        .map_err(|_| anyhow!("Bluetooth did not become ready; check that it is on and that your terminal app is allowed in System Settings > Privacy & Security > Bluetooth"))?
}

/// Turn btleplug's permission error into something the user can act on.
fn explain(e: btleplug::Error) -> anyhow::Error {
    match e {
        btleplug::Error::PermissionDenied => anyhow!(
            "no Bluetooth permission; allow your terminal app in System Settings > Privacy & Security > Bluetooth"
        ),
        other => anyhow!(other),
    }
}
