use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use crate::core::install::assets::AssetManager;
use crate::core::meta::models::AssetIndexReference;
use crate::core::retry::retry_with_backoff;

const VERSION_MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest.json";
const JRE_MANIFEST_URL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Deserialize)]
pub struct VersionManifestIndex {
    pub versions: Vec<VersionIndexItem>,
}

#[derive(Debug, Deserialize)]
pub struct VersionIndexItem {
    pub id: String,
    pub url: String,
    pub r#type: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionDetail {
    pub downloads: VersionDownloads,
    pub libraries: Option<Vec<InstallLibrary>>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersionRequirement>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndexReference>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionRequirement {
    pub component: String,
    pub major_version: u8,
}

#[derive(Debug, Deserialize)]
pub struct JreManifestIndex {
    #[serde(flatten)]
    pub platforms:
        std::collections::HashMap<String, std::collections::HashMap<String, Vec<JrePlatformEntry>>>,
}

#[derive(Debug, Deserialize)]
pub struct JrePlatformEntry {
    pub manifest: JreManifestInfo,
}

#[derive(Debug, Deserialize)]
pub struct JreManifestInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct JreComponentManifest {
    pub files: std::collections::HashMap<String, JreFile>,
}

#[derive(Debug, Deserialize)]
pub struct JreFile {
    pub r#type: String,
    pub downloads: Option<JreFileDownloads>,
    pub executable: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct JreFileDownloads {
    pub raw: Option<JreDownloadInfo>,
    pub lzma: Option<JreDownloadInfo>,
}

#[derive(Debug, Deserialize)]
pub struct JreDownloadInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct VersionDownloads {
    pub client: DownloadInfo,
}

#[derive(Debug, Deserialize)]
pub struct DownloadInfo {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InstallLibrary {
    pub name: String,
    pub downloads: Option<InstallLibraryDownloads>,
    pub rules: Option<Vec<LibraryRule>>,
    pub natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InstallLibraryDownloads {
    pub artifact: Option<LibraryArtifact>,
    pub classifiers: Option<std::collections::HashMap<String, LibraryArtifact>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LibraryArtifact {
    pub path: String,
    pub url: String,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LibraryRule {
    pub action: String,
    pub os: Option<OsCondition>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OsCondition {
    pub name: Option<String>,
}

pub async fn get_official_mc_path() -> Result<PathBuf, String> {
    if cfg!(windows) {
        Ok(PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft"))
    } else {
        let home = std::env::var("HOME").map_err(|_| "Could not find HOME directory")?;
        Ok(PathBuf::from(home).join(".minecraft"))
    }
}

/// Returns the current OS name as Minecraft knows it ("windows", "osx", "linux")
fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn get_jre_platform_id() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            "windows-x64"
        } else {
            "windows-x86"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "mac-os-arm64"
        } else {
            "mac-os"
        }
    } else {
        if cfg!(target_arch = "x86_64") {
            "linux"
        } else {
            "linux-i386"
        }
    }
}

/// Evaluates library rules: should this library be included on the current OS?
fn library_allowed(rules: &Option<Vec<LibraryRule>>) -> bool {
    let rules = match rules {
        Some(r) if !r.is_empty() => r,
        _ => return true,
    };

    let os = current_os_name();
    let mut allowed = false;

    for rule in rules {
        let matches_os = match &rule.os {
            Some(cond) => cond.name.as_deref() == Some(os),
            None => true,
        };
        if matches_os {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

async fn download_file(client: &Client, url: &str, path: &PathBuf) -> Result<bool, String> {
    if path.exists() {
        return Ok(false); // Already present
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create dir {:?}: {}", parent, e))?;
    }

    let url = url.to_string();
    let path = path.clone();
    let client = client.clone();

    retry_with_backoff(|| async {
        let mut response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP {} for {}", response.status(), url));
        }

        let mut file = fs::File::create(&path)
            .await
            .map_err(|e| format!("Failed to create file {:?}: {}", path, e))?;

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Chunk error from {}: {}", url, e))?
        {
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write error: {}", e))?;
        }

        Ok(())
    })
    .await?;

    Ok(true) // Downloaded new file
}

/// Checks if the Minecraft version is already installed (JAR and JSON exist).
pub async fn is_installed(version: &str) -> bool {
    if let Ok(mc_path) = get_official_mc_path().await {
        let version_dir = mc_path.join("versions").join(version);
        version_dir.join(format!("{}.json", version)).exists()
            && version_dir.join(format!("{}.jar", version)).exists()
    } else {
        false
    }
}

/// Installs Minecraft version: JSON, client JAR, all required libraries, JRE, and game assets.
pub async fn install_version(version: &str) -> Result<(), String> {
    install_version_with_assets(version, None, None).await
}

/// Installs Minecraft version with explicit assets directory and optional progress window.
pub async fn install_version_with_assets(
    version: &str,
    assets_dir: Option<&std::path::Path>,
    window: Option<&tauri::Window>,
) -> Result<(), String> {
    let client = Client::new();
    let mc_path = get_official_mc_path().await?;
    let version_dir = mc_path.join("versions").join(version);
    let version_json_path = version_dir.join(format!("{}.json", version));
    let version_jar_path = version_dir.join(format!("{}.jar", version));

    fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| format!("Failed to create version directory: {}", e))?;

    // ── Step 1: Fetch version manifest index ───────────────────────────────────
    let manifest: VersionManifestIndex = retry_with_backoff(|| async {
        client
            .get(VERSION_MANIFEST_URL)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch version manifest: {}", e))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse version manifest: {}", e))
    })
    .await?;

    let version_info = manifest
        .versions
        .iter()
        .find(|v| v.id == version)
        .ok_or_else(|| format!("Version {} not found in Mojang manifest", version))?;

    // ── Step 2: Download version JSON ─────────────────────────────────────────
    if !version_json_path.exists() {
        log::info!("Downloading version JSON for {}...", version);
        let content = retry_with_backoff(|| async {
            client
                .get(&version_info.url)
                .send()
                .await
                .map_err(|e| format!("Failed to fetch version JSON: {}", e))?
                .text()
                .await
                .map_err(|e| format!("Failed to read version JSON: {}", e))
        })
        .await?;

        fs::write(&version_json_path, &content)
            .await
            .map_err(|e| format!("Failed to save version JSON: {}", e))?;
    }

    // ── Step 3: Parse version JSON ────────────────────────────────────────────
    let content = fs::read_to_string(&version_json_path)
        .await
        .map_err(|e| format!("Failed to read version JSON: {}", e))?;

    let detail: VersionDetail = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse version detail: {}", e))?;

    // ── Step 4: Download client JAR ───────────────────────────────────────────
    if !version_jar_path.exists() {
        log::info!("Downloading Minecraft {} client JAR...", version);
        let jar_url = detail.downloads.client.url.clone();
        let jar_path = version_jar_path.clone();

        retry_with_backoff(|| async {
            let mut response = client
                .get(&jar_url)
                .send()
                .await
                .map_err(|e| format!("Failed to start JAR download: {}", e))?;

            let mut file = fs::File::create(&jar_path)
                .await
                .map_err(|e| format!("Failed to create JAR file: {}", e))?;

            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| format!("JAR download chunk error: {}", e))?
            {
                file.write_all(&chunk)
                    .await
                    .map_err(|e| format!("JAR write error: {}", e))?;
            }
            Ok(())
        })
        .await?;
        log::info!("Minecraft {} JAR downloaded.", version);
    }

    // ── Step 5: Download libraries ────────────────────────────────────────────
    if let Some(libraries) = detail.libraries {
        let os = current_os_name();
        let lib_base = mc_path.join("libraries");
        let mut downloaded = 0usize;
        let mut already_present = 0usize;
        let mut failed = 0usize;

        for lib in &libraries {
            if !library_allowed(&lib.rules) {
                continue;
            }

            if let Some(dl) = &lib.downloads {
                // Main artifact
                if let Some(artifact) = &dl.artifact {
                    let dest = lib_base.join(&artifact.path);
                    match download_file(&client, &artifact.url, &dest).await {
                        Ok(true) => downloaded += 1,
                        Ok(false) => already_present += 1,
                        Err(e) => {
                            log::warn!("Lib {} artifact failed: {}", lib.name, e);
                            failed += 1;
                        }
                    }
                }

                // Native for current OS
                if let (Some(natives_map), Some(classifiers)) = (&lib.natives, &dl.classifiers) {
                    if let Some(native_key) = natives_map.get(os) {
                        if let Some(native_art) = classifiers.get(native_key) {
                            let dest = lib_base.join(&native_art.path);
                            match download_file(&client, &native_art.url, &dest).await {
                                Ok(true) => downloaded += 1,
                                Ok(false) => already_present += 1,
                                Err(e) => {
                                    log::warn!(
                                        "Native {} for {} failed: {}",
                                        native_key,
                                        lib.name,
                                        e
                                    );
                                    failed += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        log::info!(
            "Libraries for {}: {} newly downloaded, {} already present, {} failed.",
            version,
            downloaded,
            already_present,
            failed
        );
    }

    // ── Step 6: Ensure required Java runtime is downloaded ───────────────────
    if let Some(req) = detail.java_version {
        download_jre(&req.component).await?;
    } else {
        // Default to jre-legacy (Java 8) for older versions if not specified
        download_jre("jre-legacy").await?;
    }

    // ── Step 7: Ensure game assets and asset index are downloaded ───────────────
    let effective_assets_dir = match assets_dir {
        Some(dir) => {
            let str_val = dir.to_string_lossy();
            if str_val.starts_with("http://")
                || str_val.starts_with("https://")
                || str_val.contains("://")
                || str_val.ends_with(".json")
            {
                mc_path.join("assets")
            } else {
                dir.to_path_buf()
            }
        }
        None => mc_path.join("assets"),
    };
    fs::create_dir_all(&effective_assets_dir)
        .await
        .map_err(|e| format!("Failed to create assets directory {:?}: {}", effective_assets_dir, e))?;
    let _ = fs::create_dir_all(effective_assets_dir.join("indexes")).await;
    let _ = fs::create_dir_all(effective_assets_dir.join("objects")).await;

    if let Some(ref asset_index_ref) = detail.asset_index {
        log::info!(
            "Ensuring game assets for {} (index: {}) in {:?}",
            version,
            asset_index_ref.id,
            effective_assets_dir
        );

        let asset_manager = AssetManager::new_with_assets_dir(effective_assets_dir);
        asset_manager.ensure_assets(window, asset_index_ref).await?;
    } else {
        let default_index = effective_assets_dir.join("indexes").join(format!("{}.json", version));
        let needs_fallback = !default_index.exists()
            || fs::metadata(&default_index).await.map(|m| m.len() == 0).unwrap_or(true)
            || fs::read_to_string(&default_index).await.map(|s| serde_json::from_str::<serde_json::Value>(&s).is_err()).unwrap_or(true);
        if needs_fallback {
            let _ = fs::write(&default_index, r#"{"objects":{}}"#).await;
        }
    }

    Ok(())
}

/// Downloads a specific Java runtime component from Mojang's manifest.
pub async fn download_jre(component: &str) -> Result<PathBuf, String> {
    let client = Client::new();
    let mc_path = get_official_mc_path().await?;
    let platform = get_jre_platform_id();

    // Path where this component will be stored: .minecraft/runtime/<component>/<platform>/<component>
    let jre_base_path = mc_path
        .join("runtime")
        .join(component)
        .join(platform)
        .join(component);
    let java_bin = if cfg!(windows) {
        jre_base_path.join("bin/java.exe")
    } else {
        jre_base_path.join("bin/java")
    };

    if java_bin.exists() {
        return Ok(jre_base_path);
    }

    log::info!(
        "Downloading Java runtime component: {} for {}",
        component,
        platform
    );

    // 1. Fetch JRE manifest index
    let index: JreManifestIndex = client
        .get(JRE_MANIFEST_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch JRE manifest index: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse JRE manifest index: {}", e))?;

    let platform_map = index
        .platforms
        .get(platform)
        .ok_or_else(|| format!("Platform {} not found in JRE manifest index", platform))?;

    let component_list = platform_map.get(component).ok_or_else(|| {
        format!(
            "Component {} not found for platform {} in JRE manifest index",
            component, platform
        )
    })?;

    let component_entry = component_list.first().ok_or_else(|| {
        format!(
            "No entries for component {} on platform {}",
            component, platform
        )
    })?;

    // 2. Fetch component manifest
    let component_manifest: JreComponentManifest = client
        .get(&component_entry.manifest.url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch JRE component manifest: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse JRE component manifest: {}", e))?;

    // 3. Download all files
    let mut downloaded = 0;
    let mut skipped = 0;
    let total_files = component_manifest.files.len();

    for (path, file) in component_manifest.files {
        if file.r#type == "directory" {
            let dir_path = jre_base_path.join(path);
            fs::create_dir_all(dir_path).await.ok();
            continue;
        }

        if file.r#type == "file" {
            if let Some(downloads) = file.downloads {
                if let Some(raw) = downloads.raw {
                    let dest_path = jre_base_path.join(&path);
                    match download_file(&client, &raw.url, &dest_path).await {
                        Ok(true) => {
                            downloaded += 1;
                            // Make executable if needed
                            #[cfg(unix)]
                            if file.executable.unwrap_or(false) || path.contains("bin/") {
                                use std::os::unix::fs::PermissionsExt;
                                if let Ok(metadata) = fs::metadata(&dest_path).await {
                                    let mut perms = metadata.permissions();
                                    perms.set_mode(0o755);
                                    fs::set_permissions(&dest_path, perms).await.ok();
                                }
                            }
                        }
                        Ok(false) => skipped += 1,
                        Err(e) => log::warn!("Failed to download JRE file {}: {}", path, e),
                    }
                }
            }
        }
    }

    log::info!(
        "JRE {} downloaded: {} new files, {} skipped out of {}.",
        component,
        downloaded,
        skipped,
        total_files
    );
    Ok(jre_base_path)
}
