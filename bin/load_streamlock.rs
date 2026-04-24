use clap::Parser;

#[tokio::main]
async fn main() {
    app_benchmarks::bin_load_streamlock::run(
        app_benchmarks::bin_load_streamlock::Args::parse(),
    )
    .await;
}
