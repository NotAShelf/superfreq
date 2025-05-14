use crate::config::watcher::ConfigWatcher;
use crate::config::{AppConfig, LogLevel};
use crate::core::SystemReport;
use crate::engine;
use crate::monitor;
use log::{LevelFilter, debug, error, info, warn};
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Run the daemon
pub fn run_daemon(mut config: AppConfig, verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    // Set effective log level based on config and verbose flag
    let effective_log_level = if verbose {
        LogLevel::Debug
    } else {
        config.daemon.log_level
    };

    // Get the appropriate level filter
    let level_filter = match effective_log_level {
        LogLevel::Error => LevelFilter::Error,
        LogLevel::Warning => LevelFilter::Warn,
        LogLevel::Info => LevelFilter::Info,
        LogLevel::Debug => LevelFilter::Debug,
    };

    // Update the log level filter if needed, without re-initializing the logger
    log::set_max_level(level_filter);

    info!("Starting superfreq daemon...");

    // Create a flag that will be set to true when a signal is received
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Set up signal handlers
    ctrlc::set_handler(move || {
        info!("Received shutdown signal, exiting...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    info!(
        "Daemon initialized with poll interval: {}s",
        config.daemon.poll_interval_sec
    );

    // Set up stats file if configured
    if let Some(stats_path) = &config.daemon.stats_file_path {
        info!("Stats will be written to: {stats_path}");
    }

    // Initialize config file watcher if a path is available
    let config_file_path = if let Ok(path) = std::env::var("SUPERFREQ_CONFIG") {
        Some(path)
    } else {
        // Check standard config paths
        let default_paths = ["/etc/xdg/superfreq/config.toml", "/etc/superfreq.toml"];

        default_paths
            .iter()
            .find(|&path| std::path::Path::new(path).exists())
            .map(|path| (*path).to_string())
    };

    let mut config_watcher = if let Some(path) = config_file_path {
        match ConfigWatcher::new(&path) {
            Ok(watcher) => {
                info!("Watching config file: {path}");
                Some(watcher)
            }
            Err(e) => {
                warn!("Failed to initialize config file watcher: {e}");
                None
            }
        }
    } else {
        warn!("No config file found to watch for changes.");
        None
    };

    // Variables for adaptive polling
    let mut current_poll_interval = config.daemon.poll_interval_sec;
    let mut last_settings_change = Instant::now();
    let mut last_system_state = SystemState::Unknown;

    // Main loop
    while running.load(Ordering::SeqCst) {
        let start_time = Instant::now();

        // Check for configuration changes
        if let Some(watcher) = &mut config_watcher {
            if let Some(config_result) = watcher.check_for_changes() {
                match config_result {
                    Ok(new_config) => {
                        info!("Config file changed, updating configuration");
                        config = new_config;
                        // Reset polling interval after config change
                        current_poll_interval = config.daemon.poll_interval_sec;
                        // Record this as a settings change for adaptive polling purposes
                        last_settings_change = Instant::now();
                    }
                    Err(e) => {
                        error!("Error loading new configuration: {e}");
                        // Continue with existing config
                    }
                }
            }
        }

        match monitor::collect_system_report(&config) {
            Ok(report) => {
                debug!("Collected system report, applying settings...");

                // Determine current system state
                let current_state = determine_system_state(&report);

                // Update the stats file if configured
                if let Some(stats_path) = &config.daemon.stats_file_path {
                    if let Err(e) = write_stats_file(stats_path, &report) {
                        error!("Failed to write stats file: {e}");
                    }
                }

                match engine::determine_and_apply_settings(&report, &config, None) {
                    Ok(()) => {
                        debug!("Successfully applied system settings");

                        // If system state changed or settings were applied differently, record the time
                        if current_state != last_system_state {
                            last_settings_change = Instant::now();
                            last_system_state = current_state.clone();

                            info!("System state changed to: {current_state:?}");
                        }
                    }
                    Err(e) => {
                        error!("Error applying system settings: {e}");
                    }
                }

                // Adjust poll interval if adaptive polling is enabled
                if config.daemon.adaptive_interval {
                    let time_since_change = last_settings_change.elapsed().as_secs();

                    // If we've been stable for a while, increase the interval (up to max)
                    if time_since_change > 60 {
                        current_poll_interval =
                            (current_poll_interval * 2).min(config.daemon.max_poll_interval_sec);

                        debug!("Adaptive polling: increasing interval to {current_poll_interval}s");
                    } else if time_since_change < 10 {
                        // If we've had recent changes, decrease the interval (down to min)
                        current_poll_interval =
                            (current_poll_interval / 2).max(config.daemon.min_poll_interval_sec);

                        debug!("Adaptive polling: decreasing interval to {current_poll_interval}s");
                    }
                } else {
                    // If not adaptive, use the configured poll interval
                    current_poll_interval = config.daemon.poll_interval_sec;
                }

                // If on battery and throttling is enabled, lengthen the poll interval to save power
                if config.daemon.throttle_on_battery
                    && !report.batteries.is_empty()
                    && report.batteries.first().is_some_and(|b| !b.ac_connected)
                {
                    let battery_multiplier = 2; // Poll half as often on battery
                    current_poll_interval = (current_poll_interval * battery_multiplier)
                        .min(config.daemon.max_poll_interval_sec);

                    debug!("On battery power, increasing poll interval to save energy");
                }
            }
            Err(e) => {
                error!("Error collecting system report: {e}");
            }
        }

        // Sleep for the remaining time in the poll interval
        let elapsed = start_time.elapsed();
        let poll_duration = Duration::from_secs(current_poll_interval);
        if elapsed < poll_duration {
            let sleep_time = poll_duration - elapsed;
            debug!("Sleeping for {}s until next cycle", sleep_time.as_secs());
            std::thread::sleep(sleep_time);
        }
    }

    info!("Daemon stopped");
    Ok(())
}

/// Write current system stats to a file for --stats to read
fn write_stats_file(path: &str, report: &SystemReport) -> Result<(), std::io::Error> {
    let mut file = File::create(path)?;

    writeln!(file, "timestamp={:?}", report.timestamp)?;

    // CPU info
    writeln!(file, "governor={:?}", report.cpu_global.current_governor)?;
    writeln!(file, "turbo={:?}", report.cpu_global.turbo_status)?;

    if let Some(temp) = report.cpu_global.average_temperature_celsius {
        writeln!(file, "cpu_temp={temp:.1}")?;
    }

    // Battery info
    if !report.batteries.is_empty() {
        let battery = &report.batteries[0];
        writeln!(file, "ac_power={}", battery.ac_connected)?;
        if let Some(cap) = battery.capacity_percent {
            writeln!(file, "battery_percent={cap}")?;
        }
    }

    // System load
    writeln!(file, "load_1m={:.2}", report.system_load.load_avg_1min)?;
    writeln!(file, "load_5m={:.2}", report.system_load.load_avg_5min)?;
    writeln!(file, "load_15m={:.2}", report.system_load.load_avg_15min)?;

    Ok(())
}

/// Simplified system state used for determining when to adjust polling interval
#[derive(Debug, PartialEq, Eq, Clone)]
enum SystemState {
    Unknown,
    OnAC,
    OnBattery,
    HighLoad,
    LowLoad,
    HighTemp,
}

/// Determine the current system state for adaptive polling
fn determine_system_state(report: &SystemReport) -> SystemState {
    // Check power state first
    if !report.batteries.is_empty() {
        if let Some(battery) = report.batteries.first() {
            if battery.ac_connected {
                return SystemState::OnAC;
            }
            return SystemState::OnBattery;
        }
    }

    // No batteries means desktop, so always AC
    if report.batteries.is_empty() {
        return SystemState::OnAC;
    }

    // Check temperature
    if let Some(temp) = report.cpu_global.average_temperature_celsius {
        if temp > 80.0 {
            return SystemState::HighTemp;
        }
    }

    // Check load
    let avg_load = report.system_load.load_avg_1min;
    if avg_load > 3.0 {
        return SystemState::HighLoad;
    }
    if avg_load < 0.5 {
        return SystemState::LowLoad;
    }

    // Default case
    SystemState::Unknown
}
