use std::time::Duration;
use tokio::sync::mpsc;

use crate::config;
use crate::events::{AppEvent, SubGhzSignal};

const YS1_VID: u16 = 0x1d50;
const YS1_PIDS: &[u16] = &[0x605b, 0x6048, 0x604f, 0x6047];

const EP_OUT: u8 = 0x05;
const EP_IN: u8 = 0x85;

// CC1111 protocol constants — kept for documentation even when unused so the
// register/strobe layout matches the chip datasheet at a glance.
#[allow(dead_code)] const APP_SYSTEM: u8 = 0xff;
#[allow(dead_code)] const SYS_CMD_PING: u8 = 0x82;
const SYS_CMD_PEEK: u8 = 0x80;
const SYS_CMD_POKE: u8 = 0x81;

// CC1111 SFR register addresses
const RFST: u16 = 0xdfe1;   // RF strobe
const RSSI: u16 = 0xdf06;   // RSSI value
const FREQ2: u16 = 0xdf09;
#[allow(dead_code)] const FREQ1: u16 = 0xdf0a; // FREQ2 cascades, we only write FREQ2
#[allow(dead_code)] const FREQ0: u16 = 0xdf0b;
const MARCSTATE: u16 = 0xdf14; // Main Radio FSM State

// RF strobes
#[allow(dead_code)] const SFSTXON: u8 = 0x00; // datasheet-completeness
const SCAL: u8 = 0x01;
const SRX: u8 = 0x02;
#[allow(dead_code)] const STX: u8 = 0x03;     // RX-only scanner; TX strobe kept for reference
const SIDLE: u8 = 0x04;

const CRYSTAL_MHZ: f64 = 24.0;
const NOISE_FLOOR_DBM: i32 = -100;

struct BandInfo { name: &'static str, start_mhz: f64, end_mhz: f64 }
const BANDS: &[BandInfo] = &[
    BandInfo { name: "315 MHz", start_mhz: 310.0, end_mhz: 320.0 },
    BandInfo { name: "433 MHz", start_mhz: 430.0, end_mhz: 436.0 },
    BandInfo { name: "868 MHz", start_mhz: 865.0, end_mhz: 870.0 },
    BandInfo { name: "915 MHz", start_mhz: 902.0, end_mhz: 928.0 },
];

fn freq_to_band(freq_mhz: f64) -> String {
    for b in BANDS {
        if freq_mhz >= b.start_mhz && freq_mhz <= b.end_mhz { return b.name.to_string(); }
    }
    format!("{:.0} MHz", freq_mhz)
}

struct YardStick {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
}

impl Drop for YardStick {
    fn drop(&mut self) {
        // Release interface and reset device so it's usable again
        let _ = self.handle.release_interface(0);
        let _ = self.handle.reset();
    }
}

impl YardStick {
    fn open() -> anyhow::Result<Self> {
        let handle = YS1_PIDS.iter()
            .find_map(|pid| rusb::open_device_with_vid_pid(YS1_VID, *pid))
            .ok_or_else(|| anyhow::anyhow!("Yard Stick One not found"))?;
        let _ = handle.detach_kernel_driver(0);

        // Reset USB device to clean state (like replugging)
        match handle.reset() {
            Ok(_) => std::thread::sleep(Duration::from_millis(500)),
            Err(_) => {
                // After reset, device may re-enumerate — reopen
                drop(handle);
                std::thread::sleep(Duration::from_millis(1000));
                let handle = YS1_PIDS.iter()
                    .find_map(|pid| rusb::open_device_with_vid_pid(YS1_VID, *pid))
                    .ok_or_else(|| anyhow::anyhow!("YS1 lost after USB reset"))?;
                let _ = handle.detach_kernel_driver(0);
                handle.claim_interface(0)?;
                let mut ys = Self { handle };
                ys.flush();
                return Ok(ys);
            }
        }

        handle.claim_interface(0)?;
        let mut ys = Self { handle };
        ys.flush();
        Ok(ys)
    }

    fn flush(&mut self) {
        let mut buf = vec![0u8; 512];
        for _ in 0..10 {
            if self.handle.read_bulk(EP_IN, &mut buf, Duration::from_millis(30)).unwrap_or(0) == 0 { break; }
        }
    }

    fn send_cmd(&self, app: u8, cmd: u8, payload: &[u8]) -> anyhow::Result<Vec<u8>> {
        let len = payload.len() as u16;
        let mut msg = vec![app, cmd];
        msg.extend_from_slice(&len.to_le_bytes());
        msg.extend_from_slice(payload);
        self.handle.write_bulk(EP_OUT, &msg, Duration::from_millis(200))?;
        std::thread::sleep(Duration::from_millis(5));
        let mut buf = vec![0u8; 512];
        let n = self.handle.read_bulk(EP_IN, &mut buf, Duration::from_millis(100)).unwrap_or(0);
        buf.truncate(n);
        Ok(buf)
    }

    /// Peek a register: payload = count(u16 LE) + addr(u16 LE)
    fn peek(&self, addr: u16, count: u16) -> anyhow::Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&count.to_le_bytes());
        payload.extend_from_slice(&addr.to_le_bytes());
        self.send_cmd(APP_SYSTEM, SYS_CMD_PEEK, &payload)
    }

    /// Poke a register: payload = addr(u16 LE) + data bytes
    fn poke(&self, addr: u16, values: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&addr.to_le_bytes());
        payload.extend_from_slice(values);
        self.send_cmd(APP_SYSTEM, SYS_CMD_POKE, &payload)
    }

    /// Set frequency by poking FREQ2/1/0
    fn set_freq(&self, freq_hz: u32) -> anyhow::Result<()> {
        let freqmult = (0x10000 as f64 / 1_000_000.0) / CRYSTAL_MHZ;
        let num = (freq_hz as f64 * freqmult) as u32;
        self.poke(FREQ2, &[
            (num >> 16) as u8,
            ((num >> 8) & 0xff) as u8,
            (num & 0xff) as u8,
        ])?;
        Ok(())
    }

    /// Strobe the radio state machine
    fn strobe(&self, cmd: u8) -> anyhow::Result<()> {
        self.poke(RFST, &[cmd])?;
        Ok(())
    }

    /// Read RSSI register
    fn read_rssi(&self) -> anyhow::Result<i32> {
        let resp = self.peek(RSSI, 1)?;
        // Response: '@' APP CMD LEN_LO LEN_HI [data...]
        let data_start = if resp.len() > 5 && resp[0] == 0x40 { 5 } else if resp.len() > 4 { 4 } else { 0 };
        if resp.len() > data_start {
            let raw = resp[data_start];
            Ok(((raw as i8) as i32) - 74) // CC1111: RSSI_dBm = RSSI_val - RSSI_offset (74)
        } else {
            Err(anyhow::anyhow!("empty RSSI response"))
        }
    }

    /// Measure signal at a specific frequency
    fn measure_freq(&self, freq_hz: u32) -> anyhow::Result<i32> {
        self.strobe(SIDLE)?;
        self.set_freq(freq_hz)?;
        self.strobe(SCAL)?;    // calibrate
        std::thread::sleep(Duration::from_millis(1));
        self.strobe(SRX)?;     // enter RX
        std::thread::sleep(Duration::from_millis(3)); // wait for RSSI to settle
        let rssi = self.read_rssi()?;
        self.strobe(SIDLE)?;
        Ok(rssi)
    }

    /// Liveness check via SYS_CMD_PING — currently unused (we use the MARCSTATE
    /// peek result as our liveness signal during scanner startup).
    #[allow(dead_code)]
    fn ping(&self) -> bool {
        self.send_cmd(APP_SYSTEM, SYS_CMD_PING, b"ABCDEFGHIJKLMNOPQRSTUVWXYZ")
            .map(|r| !r.is_empty())
            .unwrap_or(false)
    }
}

struct ScanRange {
    label: &'static str,
    start_hz: u32,
    end_hz: u32,
    step_hz: u32,
}

const SCAN_RANGES: &[ScanRange] = &[
    ScanRange { label: "315 MHz", start_hz: 312_000_000, end_hz: 318_000_000, step_hz: 200_000 },
    ScanRange { label: "433 MHz", start_hz: 430_000_000, end_hz: 435_000_000, step_hz: 100_000 },
    ScanRange { label: "868 MHz", start_hz: 866_000_000, end_hz: 870_000_000, step_hz: 200_000 },
    ScanRange { label: "915 MHz", start_hz: 902_000_000, end_hz: 928_000_000, step_hz: 500_000 },
];

pub async fn run_subghz_scanner(tx: mpsc::Sender<AppEvent>) -> anyhow::Result<()> {
    let ys = match YardStick::open() {
        Ok(ys) => ys,
        Err(e) => {
            tx.send(AppEvent::SubGhzStatus(format!("{}", e))).await.ok();
            return Ok(());
        }
    };

    // Test: try to peek a register
    let diag = match ys.peek(MARCSTATE, 1) {
        Ok(resp) if !resp.is_empty() => {
            let hex: String = resp.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
            format!("YS1 connected (peek OK: [{}]) — scanning", hex)
        }
        Ok(_) => "YS1 connected (peek empty) — scanning".into(),
        Err(e) => format!("YS1 peek failed: {} — scanning anyway", e),
    };

    tx.send(AppEvent::SubGhzStatus(diag)).await.ok();

    let tx_clone = tx.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let tx = tx_clone;

        loop {
            for range in SCAN_RANGES {
                let mut freq = range.start_hz;
                while freq <= range.end_hz {
                    // Stop if channel closed (app quitting)
                    if tx.is_closed() {
                        drop(ys); // triggers USB cleanup
                        return;
                    }
                    if let Ok(rssi_dbm) = ys.measure_freq(freq) {
                        if rssi_dbm > NOISE_FLOOR_DBM {
                            let freq_mhz = freq as f64 / 1_000_000.0;
                            if tx.blocking_send(AppEvent::SubGhzSignal(SubGhzSignal {
                                freq_hz: freq,
                                rssi_dbm: rssi_dbm as i16,
                                band: freq_to_band(freq_mhz),
                            })).is_err() {
                                drop(ys);
                                return;
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_micros(config::SUBGHZ_STEP_DELAY_US));
                    freq += range.step_hz;
                }
            }
        }
    });

    handle.await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_freq_to_ism_band() {
        assert_eq!(freq_to_band(433.92), "433 MHz");
        assert_eq!(freq_to_band(315.0), "315 MHz");
        assert_eq!(freq_to_band(868.3), "868 MHz");
        assert_eq!(freq_to_band(915.0), "915 MHz");
        assert_eq!(freq_to_band(100.0), "100 MHz"); // outside known bands -> fallback
    }
}
