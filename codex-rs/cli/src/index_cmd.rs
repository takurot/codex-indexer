use clap::Parser;
use codex_common::CliConfigOverrides;
use codex_core::AuthManager;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::semantic::index::SemanticIndex;
use std::sync::Arc;

#[derive(Debug, Parser)]
pub(crate) struct IndexCommand {
    #[command(subcommand)]
    pub(crate) subcommand: IndexSubcommand,

    #[clap(flatten)]
    pub(crate) config_overrides: CliConfigOverrides,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum IndexSubcommand {
    /// Build the semantic index for this workspace.
    Build,
    /// Show semantic index stats.
    Stats,
    /// Clear the semantic index for this workspace.
    Clear,
}

pub(crate) async fn run_index_command(cmd: IndexCommand) -> anyhow::Result<()> {
    let cli_overrides = cmd
        .config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let config = Config::load_with_cli_overrides_and_harness_overrides(
        cli_overrides,
        ConfigOverrides::default(),
    )
    .await?;

    let auth_manager = Arc::new(AuthManager::new(
        config.codex_home.clone(),
        false,
        config.cli_auth_credentials_store_mode,
    ));
    let index = SemanticIndex::new(
        config.cwd.clone(),
        config.semantic_index.clone(),
        config.model_provider.clone(),
        Some(auth_manager),
    );

    match cmd.subcommand {
        IndexSubcommand::Build => {
            let stats = index.build().await?;
            println!("Index dir: {}", config.semantic_index.dir.display());
            println!("Files: {}", stats.file_count);
            println!("Chunks: {}", stats.chunk_count);
            if let Some(model) = stats.embedding_model {
                println!("Embedding model: {model}");
            }
        }
        IndexSubcommand::Stats => {
            let stats = index.stats()?;
            println!("Index dir: {}", config.semantic_index.dir.display());
            println!("Files: {}", stats.file_count);
            println!("Chunks: {}", stats.chunk_count);
            if let Some(model) = stats.embedding_model {
                println!("Embedding model: {model}");
            }
            if let Some(dim) = stats.embedding_dim {
                println!("Embedding dim: {dim}");
            }
            if let Some(created_at) = stats.created_at {
                println!("Created at: {}", created_at.to_rfc3339());
            }
        }
        IndexSubcommand::Clear => {
            index.clear()?;
            println!("Index cleared");
        }
    }

    Ok(())
}
