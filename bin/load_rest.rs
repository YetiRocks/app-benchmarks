use clap::Parser;

#[tokio::main]
async fn main() {
    app_benchmarks::bin_load_rest::run(app_benchmarks::cli::BenchArgs::parse()).await;
}
