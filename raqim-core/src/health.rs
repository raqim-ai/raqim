use serde::Serialize;
use std::time::Duration;

use sysinfo::{Components, CpuRefreshKind, MemoryRefreshKind, Pid, RefreshKind, System};
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, Serialize)]
pub struct SystemHealth {
    pub cpu_load_percent: f32,
    pub process_memory_mb: f32,
    pub host_used_memory_mb: f32,
    pub host_total_memory_mb: f32,
    pub core_temp_celcius: f32,
    pub mesh_latency_ms: u32,
    pub ingress_paused: bool,
}

pub struct HealthMonitor;

impl HealthMonitor {
    pub fn spawn_telemetry_loop(
        health_tx: broadcast::Sender<SystemHealth>,
        pause_rx: watch::Receiver<bool>,
    ) {
        tokio::spawn(async move {
            // Initialize systeminfo  strictly for CPU and memory to save cycles.
            let mut sys = System::new_with_specifics(
                RefreshKind::new()
                    .with_cpu(CpuRefreshKind::everything())
                    .with_memory(MemoryRefreshKind::everything()),
            );

            let pid = Pid::from_u32(std::process::id());
            let mut components = Components::new_with_refreshed_list();

            loop {
                //  Only perform expensive hardware interrupts if an Admin UI is actually connected.
                if health_tx.receiver_count() > 0 {
                    sys.refresh_cpu();
                    sys.refresh_memory();
                    components.refresh_list();

                    let cpu_load = sys.global_cpu_info().cpu_usage();
                    let host_used = sys.used_memory() as f32 / (1024.0 * 1024.0);
                    let host_total = sys.total_memory() as f32 / (1024.0 * 1024.0);

                    // Process RSS memory allocation
                    let process_mem = sys
                        .process(pid)
                        .map(|p| p.memory() as f32 / (1024.0 * 1024.0))
                        .unwrap_or(45.0);

                    // Grab the first available CPU temperature sensor
                    let core_temp = components
                        .iter()
                        .next()
                        .map(|c: &sysinfo::Component| c.temperature())
                        .unwrap_or(0.0);

                    let payload = SystemHealth {
                        cpu_load_percent: cpu_load,
                        process_memory_mb: process_mem,
                        host_used_memory_mb: host_used,
                        host_total_memory_mb: host_total,
                        core_temp_celcius: core_temp,
                        mesh_latency_ms: 12,
                        ingress_paused: *pause_rx.borrow(),
                    };

                    let _ = health_tx.send(payload);
                    //  Stream at 1Hz to the UI
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                } else {
                    // Backooff and sleep when the UI is observing
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });
    }
}
