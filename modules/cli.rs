use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(about = "Yeti benchmark load test")]
pub struct BenchArgs {
    /// Test ID to run (e.g. rest-read, graphql-mutation)
    #[arg(long)]
    pub test: String,

    /// Test duration in seconds
    #[arg(long, default_value = "30")]
    pub duration: u64,

    /// Number of virtual users (concurrent tasks)
    #[arg(long, default_value = "50")]
    pub vus: u64,

    /// Base URL of the Yeti server
    #[arg(long, default_value = "https://localhost")]
    pub base_url: String,

    /// Basic auth credentials (user:pass)
    #[arg(long, default_value = "admin:admin123")]
    pub auth: String,

    /// Warmup duration in seconds (metrics are discarded during warmup)
    #[arg(long, default_value = "5")]
    pub warmup: u64,

    /// Test mode: "standard" for fixed VU count, "ramp" for progressive scaling
    #[arg(long, default_value = "standard")]
    pub mode: String,

    /// Starting VU count for ramp mode
    #[arg(long, default_value = "10")]
    pub start_vus: u64,

    /// Number of VUs to add at each ramp step
    #[arg(long, default_value = "10")]
    pub step_vus: u64,

    /// Seconds between ramp steps
    #[arg(long, default_value = "5")]
    pub step_interval: u64,

    /// Maximum VU count for ramp mode
    #[arg(long, default_value = "200")]
    pub max_vus: u64,

    /// URL to POST results to (defaults to https://localhost, i.e. the local server)
    #[arg(long, default_value = "https://localhost")]
    pub report_url: String,

    /// Path to a file where the binary writes its current phase (seeding/warming/running/cleaning)
    #[arg(long)]
    pub status_file: Option<String>,
}

/// Write the current phase to the status file (if configured).
pub fn write_phase(args: &BenchArgs, phase: &str) {
    if let Some(ref path) = args.status_file {
        let _ = std::fs::write(path, phase);
    }
}

impl BenchArgs {
    pub fn auth_parts(&self) -> (&str, &str) {
        match self.auth.split_once(':') {
            Some((user, pass)) => (user, pass),
            None => (&self.auth, ""),
        }
    }

    /// First URL from comma-separated list (for seeding/cleanup)
    pub fn primary_url(&self) -> &str {
        self.base_url.split(',').next().unwrap_or(&self.base_url).trim()
    }

    pub fn is_ramp(&self) -> bool {
        self.mode == "ramp"
    }
}
