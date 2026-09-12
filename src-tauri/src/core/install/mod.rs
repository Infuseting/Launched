pub mod assets;
pub mod fabric;
pub mod forge;
pub mod mojang;
pub mod runtime;

pub use assets::AssetManager;

use async_trait::async_trait;
use crate::core::session::Session;
use tauri::Emitter;

pub struct InstallService;

#[async_trait]
trait LoaderInstaller: Send + Sync {
    fn loader_name(&self) -> &'static str;
    fn requested_version(&self, session: &Session) -> Option<String>;
    async fn install(&self, session: &Session, version: &str) -> Result<(), String>;
}

struct ForgeInstaller;

#[async_trait]
impl LoaderInstaller for ForgeInstaller {
    fn loader_name(&self) -> &'static str {
        "Forge"
    }

    fn requested_version(&self, session: &Session) -> Option<String> {
        session.forge.clone()
    }

    async fn install(&self, session: &Session, version: &str) -> Result<(), String> {
        forge::install_forge(&session.minecraft, version).await
    }
}

struct FabricInstaller;

#[async_trait]
impl LoaderInstaller for FabricInstaller {
    fn loader_name(&self) -> &'static str {
        "Fabric"
    }

    fn requested_version(&self, session: &Session) -> Option<String> {
        session.fabric.clone()
    }

    async fn install(&self, session: &Session, version: &str) -> Result<(), String> {
        fabric::install_fabric(&session.minecraft, version).await
    }
}

fn loader_installers() -> Vec<Box<dyn LoaderInstaller>> {
    vec![Box::new(ForgeInstaller), Box::new(FabricInstaller)]
}

impl InstallService {
    pub async fn install_for_session(
        &self,
        window: &tauri::Window,
        session: &Session,
    ) -> Result<(), String> {
        let official_mc_path = mojang::get_official_mc_path().await?;
        let assets_dir = crate::core::launch::args::LaunchArguments::resolve_assets_dir(session, &official_mc_path);

        // 1. Install Minecraft Base and Assets
        let _ = window.emit(
            "sync-progress",
            crate::core::sync::SyncProgress {
                current_file: format!("Installing Minecraft {}...", session.minecraft),
                files_done: 0,
                total_files: 100,
                percentage: 0.0,
            },
        );
        mojang::install_version_with_assets(&session.minecraft, Some(&assets_dir), Some(window)).await?;

        // 2. Install selected mod loaders using a strategy list.
        for installer in loader_installers() {
            if let Some(version) = installer.requested_version(session) {
                let _ = window.emit(
                    "sync-progress",
                    crate::core::sync::SyncProgress {
                        current_file: format!(
                            "Installing {} {}...",
                            installer.loader_name(),
                            version
                        ),
                        files_done: 0,
                        total_files: 100,
                        percentage: 0.0,
                    },
                );
                installer.install(session, &version).await?;
            }
        }

        let _ = window.emit(
            "sync-progress",
            crate::core::sync::SyncProgress {
                current_file: "Installation complete".to_string(),
                files_done: 100,
                total_files: 100,
                percentage: 100.0,
            },
        );

        Ok(())
    }
}
