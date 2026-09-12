use serde::{Deserialize, Serialize};

/**
 * Represents a session configuration from servers.json.
 */
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionLink {
    pub name: String,
    pub url: String,
    pub icon: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSpec {
    pub uid: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub name: String,
    pub minecraft: String,
    pub forge: Option<String>,
    pub fabric: Option<String>,
    pub neoforge: Option<String>,
    pub quilt: Option<String>,
    pub components: Option<Vec<ComponentSpec>>,
    pub sync_dir: String,
    pub sync_url: String,
    pub welcome: String,
    pub jvm_arg: String,
    pub credits: String,
    #[serde(alias = "assets_path")]
    pub assets_path: Option<String>,
    pub hostname: Option<String>,
    pub is_default: bool,
    pub links: Option<Vec<SessionLink>>,
    #[serde(default)]
    pub crack: Option<bool>,
}

impl Session {
    pub fn effective_components(&self) -> Vec<ComponentSpec> {
        if let Some(custom) = &self.components {
            return custom.clone();
        }

        let mut components = vec![ComponentSpec {
            uid: "net.minecraft".to_string(),
            version: self.minecraft.clone(),
        }];

        if let Some(forge_ver) = &self.forge.as_deref().filter(|v| !v.trim().is_empty()) {
            components.push(ComponentSpec {
                uid: "net.minecraftforge".to_string(),
                version: forge_ver.to_string(),
            });
        } else if let Some(neoforge_ver) = &self.neoforge.as_deref().filter(|v| !v.trim().is_empty()) {
            components.push(ComponentSpec {
                uid: "net.neoforged".to_string(),
                version: neoforge_ver.to_string(),
            });
        } else if let Some(fabric_ver) = &self.fabric.as_deref().filter(|v| !v.trim().is_empty()) {
            components.push(ComponentSpec {
                uid: "net.fabricmc.fabric-loader".to_string(),
                version: fabric_ver.to_string(),
            });
        } else if let Some(quilt_ver) = &self.quilt.as_deref().filter(|v| !v.trim().is_empty()) {
            components.push(ComponentSpec {
                uid: "org.quiltmc.quilt-loader".to_string(),
                version: quilt_ver.to_string(),
            });
        }

        components
    }
}

/**
 * Handles fetching and managing sessions.
 */
pub struct SessionManager;

impl SessionManager {
    /**
     * Fetches sessions from the remote servers.json URL.
     */
    pub async fn fetch_sessions(url: &str) -> Result<Vec<Session>, String> {
        reqwest::get(url)
            .await
            .map_err(|e| e.to_string())?
            .json::<Vec<Session>>()
            .await
            .map_err(|e| e.to_string())
    }
}
