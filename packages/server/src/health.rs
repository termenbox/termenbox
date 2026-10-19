//! Server health collection (U14): samples CPU, RAM, disk and RTT every 10 s, keeps a 6-hour ring buffer.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, broadcast};
use tokio::time::interval;

/// One health sample.
#[derive(Clone, Debug)]
pub struct HealthSample {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    pub disk_percent: f32,
    pub rtt_ms: u32,
    pub ts: u64,
}

/// Ring buffer of health samples (6 hours at 10 s interval = 2160 samples).
const RING_SIZE: usize = 6 * 60 * 6; // 2160

/// Health thresholds: a server is unhealthy when any metric exceeds its threshold.
#[derive(Clone, Debug)]
pub struct HealthThresholds {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    pub disk_percent: f32,
    pub rtt_ms: u32,
}

impl Default for HealthThresholds {
    fn default() -> Self {
        Self {
            cpu_percent: 80.0,
            ram_percent: 85.0,
            disk_percent: 90.0,
            rtt_ms: 500,
        }
    }
}

/// Aggregated health status for a server.
#[derive(Clone, Debug)]
pub struct HealthStatus {
    pub healthy: bool,
    pub latest: Option<HealthSample>,
    pub thresholds: HealthThresholds,
}

/// Collects system health metrics and broadcasts them.
#[derive(Clone)]
pub struct HealthCollector {
    ring: Arc<Mutex<Vec<HealthSample>>>,
    thresholds: HealthThresholds,
    tx: broadcast::Sender<HealthSample>,
    // RTT tracking
    rtt_samples: Arc<Mutex<Vec<u32>>>,
}

impl HealthCollector {
    pub fn new(thresholds: HealthThresholds) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            ring: Arc::new(Mutex::new(Vec::with_capacity(RING_SIZE))),
            thresholds,
            tx,
            rtt_samples: Arc::new(Mutex::new(Vec::with_capacity(60))), // last 60 RTT samples (10 min)
        }
    }

    /// Subscribe to health samples.
    pub fn subscribe(&self) -> broadcast::Receiver<HealthSample> {
        self.tx.subscribe()
    }

    /// Record an RTT measurement (in ms).
    pub async fn record_rtt(&self, rtt_ms: u32) {
        let mut samples = self.rtt_samples.lock().await;
        samples.push(rtt_ms);
        if samples.len() > 60 {
            samples.remove(0);
        }
    }

    /// Get the current median RTT.
    async fn current_rtt(&self) -> u32 {
        let samples = self.rtt_samples.lock().await;
        if samples.is_empty() {
            return 0;
        }
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    /// Sample CPU, RAM, disk from /proc.
    async fn sample_system() -> (f32, f32, f32) {
        let cpu = read_cpu_percent().await;
        let ram = read_ram_percent().await;
        let disk = read_disk_percent().await;
        (cpu, ram, disk)
    }

    /// Start the collection loop (runs every 10 seconds).
    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                let (cpu, ram, disk) = Self::sample_system().await;
                let rtt = self.current_rtt().await;
                let ts = now_ms();

                let sample = HealthSample {
                    cpu_percent: cpu,
                    ram_percent: ram,
                    disk_percent: disk,
                    rtt_ms: rtt,
                    ts,
                };

                // Add to ring buffer
                {
                    let mut ring = self.ring.lock().await;
                    ring.push(sample.clone());
                    if ring.len() > RING_SIZE {
                        ring.remove(0);
                    }
                }

                // Broadcast to subscribers
                let _ = self.tx.send(sample);
            }
        });
    }

    /// Get the current health status.
    pub async fn status(&self) -> HealthStatus {
        let ring = self.ring.lock().await;
        let latest = ring.last().cloned();
        let healthy = latest.map_or(true, |s| self.is_healthy(&s));
        HealthStatus {
            healthy,
            latest,
            thresholds: self.thresholds.clone(),
        }
    }

    /// Check if a sample is healthy.
    fn is_healthy(&self, s: &HealthSample) -> bool {
        s.cpu_percent <= self.thresholds.cpu_percent
            && s.ram_percent <= self.thresholds.ram_percent
            && s.disk_percent <= self.thresholds.disk_percent
            && s.rtt_ms <= self.thresholds.rtt_ms
    }

    /// Get all samples in the ring (for debugging/inspection).
    pub async fn all_samples(&self) -> Vec<HealthSample> {
        self.ring.lock().await.clone()
    }
}

/// Current time in ms since epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Read CPU usage from /proc/stat.
async fn read_cpu_percent() -> f32 {
    let stat1 = read_proc_stat().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stat2 = read_proc_stat().await;

    let (idle1, total1) = cpu_times(&stat1);
    let (idle2, total2) = cpu_times(&stat2);

    let idle_delta = idle2.saturating_sub(idle1) as f32;
    let total_delta = total2.saturating_sub(total1) as f32;

    if total_delta > 0.0 {
        100.0 * (1.0 - idle_delta / total_delta)
    } else {
        0.0
    }
}

fn cpu_times(stat: &str) -> (u64, u64) {
    let line = stat.lines().next().unwrap_or("");
    let parts: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    let total: u64 = parts.iter().sum();
    let idle = parts.get(3).copied().unwrap_or(0); // idle is the 4th field
    (idle, total)
}

async fn read_proc_stat() -> String {
    tokio::fs::read_to_string("/proc/stat")
        .await
        .unwrap_or_default()
}

/// Read RAM usage from /proc/meminfo.
async fn read_ram_percent() -> f32 {
    let meminfo = tokio::fs::read_to_string("/proc/meminfo")
        .await
        .unwrap_or_default();

    let mut total = 0u64;
    let mut available = 0u64;

    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("MemAvailable:") {
            available = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    if total > 0 {
        100.0 * (1.0 - available as f32 / total as f32)
    } else {
        0.0
    }
}

/// Read disk usage for the root filesystem.
async fn read_disk_percent() -> f32 {
    use std::os::unix::fs::MetadataExt;
    let output = tokio::process::Command::new("df")
        .args(["-P", "/"])
        .output()
        .await
        .ok();

    let stdout = output.and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
    let lines: Vec<&str> = stdout.lines().collect();
    if lines.len() < 2 {
        return 0.0;
    }
    let parts: Vec<&str> = lines[1].split_whitespace().collect();
    if parts.len() < 5 {
        return 0.0;
    }
    parts[4]
        .trim_end_matches('%')
        .parse::<f32>()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ring_buffer_bounds_at_2160() {
        let collector = HealthCollector::new(HealthThresholds::default());
        let arc = Arc::new(collector);

        // Fill beyond capacity
        for i in 0..2500 {
            let sample = HealthSample {
                cpu_percent: 10.0,
                ram_percent: 20.0,
                disk_percent: 30.0,
                rtt_ms: 50,
                ts: i as u64,
            };
            {
                let mut ring = arc.ring.lock().await;
                ring.push(sample);
                if ring.len() > RING_SIZE {
                    ring.remove(0);
                }
            }
        }

        let ring = arc.ring.lock().await;
        assert_eq!(ring.len(), RING_SIZE);
        // Oldest should be at index 0 with ts around 340 (2500 - 2160)
        assert!(ring[0].ts >= 340);
    }

    #[tokio::test]
    async fn health_thresholds_work() {
        let thresholds = HealthThresholds {
            cpu_percent: 50.0,
            ram_percent: 50.0,
            disk_percent: 50.0,
            rtt_ms: 100,
        };
        let collector = HealthCollector::new(thresholds);

        // Healthy sample
        let healthy = HealthSample {
            cpu_percent: 10.0,
            ram_percent: 20.0,
            disk_percent: 30.0,
            rtt_ms: 50,
            ts: 1,
        };
        assert!(collector.is_healthy(&healthy));

        // Unhealthy CPU
        let unhealthy_cpu = HealthSample {
            cpu_percent: 60.0,
            ram_percent: 20.0,
            disk_percent: 30.0,
            rtt_ms: 50,
            ts: 2,
        };
        assert!(!collector.is_healthy(&unhealthy_cpu));

        // Unhealthy RAM
        let unhealthy_ram = HealthSample {
            cpu_percent: 10.0,
            ram_percent: 60.0,
            disk_percent: 30.0,
            rtt_ms: 50,
            ts: 3,
        };
        assert!(!collector.is_healthy(&unhealthy_ram));

        // Unhealthy disk
        let unhealthy_disk = HealthSample {
            cpu_percent: 10.0,
            ram_percent: 20.0,
            disk_percent: 60.0,
            rtt_ms: 50,
            ts: 4,
        };
        assert!(!collector.is_healthy(&unhealthy_disk));

        // Unhealthy RTT
        let unhealthy_rtt = HealthSample {
            cpu_percent: 10.0,
            ram_percent: 20.0,
            disk_percent: 30.0,
            rtt_ms: 200,
            ts: 5,
        };
        assert!(!collector.is_healthy(&unhealthy_rtt));
    }

    #[tokio::test]
    async fn rtt_median_calculation() {
        let collector = HealthCollector::new(HealthThresholds::default());
        let arc = Arc::new(collector);

        arc.record_rtt(100).await;
        arc.record_rtt(200).await;
        arc.record_rtt(50).await;
        // median of [50, 100, 200] is 100
        let rtt = arc.current_rtt().await;
        assert_eq!(rtt, 100);
    }

    #[tokio::test]
    async fn status_reports_healthy_when_ring_empty() {
        let collector = HealthCollector::new(HealthThresholds::default());
        let arc = Arc::new(collector);

        let status = arc.status().await;
        assert!(status.healthy);
        assert!(status.latest.is_none());
    }
}