use clap::Parser;
use codex_common::CliConfigOverrides;
use codex_core::cache::manager::CacheManager;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;

#[derive(Debug, Parser)]
pub(crate) struct CacheCommand {
    #[command(subcommand)]
    pub(crate) subcommand: CacheSubcommand,

    #[clap(flatten)]
    pub(crate) config_overrides: CliConfigOverrides,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum CacheSubcommand {
    /// Show cache status.
    Status,
    /// Clear all cached entries.
    Clear,
}

pub(crate) async fn run_cache_command(cmd: CacheCommand) -> anyhow::Result<()> {
    let cli_overrides = cmd
        .config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let config = Config::load_with_cli_overrides_and_harness_overrides(
        cli_overrides,
        ConfigOverrides::default(),
    )
    .await?;
    let cache_manager = CacheManager::new(config.cache.clone())?;

    match cmd.subcommand {
        CacheSubcommand::Status => {
            let status = cache_manager.status()?;
            println!("Cache enabled: {}", status.enabled);
            println!("Cache dir: {}", status.dir.display());
            println!("Entries: {}", status.stats.entries);
            println!("Size bytes: {}", status.stats.total_bytes);
            println!("Max bytes: {}", status.max_bytes);
            match status.telemetry.hit_rate {
                Some(rate) => println!("Hit rate: {:.1}%", rate * 100.0),
                None => println!("Hit rate: n/a"),
            }
        }
        CacheSubcommand::Clear => {
            cache_manager.clear()?;
            println!("Cache cleared");
        }
    }

    Ok(())
}
