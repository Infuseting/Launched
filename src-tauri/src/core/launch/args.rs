use crate::auth::AuthResponse;
use crate::core::launch::models::{LibraryRule, VersionManifest};
use crate::core::session::Session;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use which::which;

pub struct LaunchArguments {
    pub java_path: PathBuf,
    pub java_home: Option<PathBuf>,
    pub game_dir: PathBuf,
    pub jvm_args: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub minecraft_args: Vec<String>,
    pub wrapper_command: String,
}

/// Returns the current OS name as Minecraft knows it.
fn current_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Evaluates library rules to check if it should be included on the current OS.
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

fn argument_rules_allow(arg: &Value) -> bool {
    let Some(rules) = arg.get("rules").and_then(|r| r.as_array()) else {
        return true;
    };

    let os = current_os_name();
    let mut allowed = false;

    for rule in rules {
        let action = rule
            .get("action")
            .and_then(|a| a.as_str())
            .unwrap_or("disallow");

        // 1. Check OS conditions
        let matches_os = if let Some(os_obj) = rule.get("os") {
            let mut ok = true;
            if let Some(name) = os_obj.get("name").and_then(|n| n.as_str()) {
                if name != os {
                    ok = false;
                }
            }
            if let Some(arch) = os_obj.get("arch").and_then(|a| a.as_str()) {
                let current_arch = if cfg!(target_arch = "x86_64") {
                    "x64"
                } else if cfg!(target_arch = "x86") {
                    "x86"
                } else if cfg!(target_arch = "aarch64") {
                    "arm64"
                } else {
                    "unknown"
                };
                if arch != current_arch && arch != "x86" {
                    ok = false;
                }
            }
            ok
        } else {
            true
        };

        // 2. Check Features conditions (Mojang argument flags)
        let matches_features = if let Some(features) = rule.get("features").and_then(|f| f.as_object()) {
            let mut ok = true;
            for (key, val) in features {
                let required = val.as_bool().unwrap_or(false);
                let our_value = match key.as_str() {
                    "is_demo_user" => false, // Launcher never runs demo user
                    "has_custom_resolution" => true,
                    "has_quick_plays_support"
                    | "is_quick_play_singleplayer"
                    | "is_quick_play_multiplayer"
                    | "is_quick_play_realms" => false, // Quick play is not used
                    _ => false,
                };
                if required != our_value {
                    ok = false;
                    break;
                }
            }
            ok
        } else {
            true
        };

        if matches_os && matches_features {
            allowed = action == "allow";
        }
    }

    allowed
}

fn argument_value_strings(arg: &Value) -> Vec<String> {
    if !argument_rules_allow(arg) {
        return Vec::new();
    }

    let value = arg.get("value").unwrap_or(arg);
    if let Some(s) = value.as_str() {
        return vec![s.to_string()];
    }

    if let Some(arr) = value.as_array() {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
    }

    Vec::new()
}

fn apply_placeholders(input: &str, replacements: &[(&str, String)]) -> String {
    let mut out = input.to_string();
    for (needle, value) in replacements {
        out = out.replace(needle, value);
    }
    out
}

/// Extracts a native JAR's .so/.dll/.dylib files into `natives_dir`.
fn extract_natives(jar_path: &Path, natives_dir: &Path) -> Result<(), String> {
    let file = fs::File::open(jar_path)
        .map_err(|e| format!("Failed to open native JAR {:?}: {}", jar_path, e))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read ZIP {:?}: {}", jar_path, e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP entry error: {}", e))?;

        let name = entry.name().to_string();

        // Only extract native library files, skip META-INF
        let is_native = name.ends_with(".so")
            || name.ends_with(".dll")
            || name.ends_with(".dylib")
            || name.ends_with(".jnilib");

        if !is_native || name.contains("META-INF") {
            continue;
        }

        // Use only the filename (strip any path inside the JAR)
        let filename = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let dest = natives_dir.join(&filename);

        if !dest.exists() {
            let mut out = fs::File::create(&dest)
                .map_err(|e| format!("Failed to create native file {:?}: {}", dest, e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("Failed to extract native {}: {}", name, e))?;
            log::info!("Extracted native: {}", filename);
        }
    }

    Ok(())
}

fn ensure_pointblank_effect_hotfix(session_dir: &Path) {
    let effects_dir = session_dir
        .join("pointblank")
        .join("STALKER_PACK")
        .join("assets")
        .join("pointblank")
        .join("effects");

    if !effects_dir.exists() {
        return;
    }

    let required_effects = ["s_gauss_flash"];
    for effect_name in required_effects {
        let effect_file = effects_dir.join(format!("{}.json", effect_name));
        if effect_file.exists() {
            continue;
        }

        let content = format!(
            "{{\n  \"type\": \"muzzle_flash\",\n  \"name\": \"{}\",\n  \"texture\": \"textures/effect/{}.png\",\n  \"blendMode\": \"ADDITIVE\",\n  \"width\": 1,\n  \"duration\": 50,\n  \"sprites\": {{\n    \"type\": \"random\",\n    \"rows\": 1,\n    \"colums\": 1,\n    \"fps\": 60\n  }}\n}}\n",
            effect_name, effect_name
        );

        if let Err(e) = fs::write(&effect_file, content) {
            log::warn!("Failed to write PointBlank effect hotfix {:?}: {}", effect_file, e);
        } else {
            log::info!("Applied PointBlank effect hotfix: {:?}", effect_file);
        }
    }
}

/// Finds the best Java binary for the given required major version.
/// Prefers exact match, falls back to any available java.
fn find_java(
    required_major: Option<u8>,
    mc_path: &Path,
) -> Result<(u8, PathBuf, Option<PathBuf>), String> {
    let mut search_paths = Vec::new();

    // 1. Check Mojang's downloaded runtimes in .minecraft/runtime
    let runtime_base = mc_path.join("runtime");
    if runtime_base.exists() {
        // Walk the runtime directory to find bin/java
        // Structure: runtime/<component>/<platform>/<component>/bin/java
        if let Ok(components) = std::fs::read_dir(&runtime_base) {
            for component in components.flatten() {
                if let Ok(platforms) = std::fs::read_dir(component.path()) {
                    for platform in platforms.flatten() {
                        if let Ok(variants) = std::fs::read_dir(platform.path()) {
                            for variant in variants.flatten() {
                                let java_home = variant.path();
                                let java_bin = if cfg!(windows) {
                                    java_home.join("bin/java.exe")
                                } else {
                                    java_home.join("bin/java")
                                };
                                if java_bin.exists() {
                                    search_paths.push(java_home);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Check system JVM locations
    #[cfg(unix)]
    {
        let jvm_base = PathBuf::from("/usr/lib/jvm");
        if let Ok(entries) = std::fs::read_dir(&jvm_base) {
            for entry in entries.flatten() {
                let path = entry.path();
                let java_bin = path.join("bin/java");
                if java_bin.exists() {
                    search_paths.push(path);
                }
            }
        }
    }

    let mut candidates: Vec<(u8, PathBuf)> = Vec::new();
    for path in search_paths {
        // Try to parse major version from directory name or binary
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        // Common patterns: java-8-openjdk, java-1.8.0-openjdk, jre-legacy, java-runtime-alpha
        let major = if name.contains("-1.8")
            || name.contains("-8-")
            || name.ends_with("-8")
            || name.contains(".1.8")
            || name.contains("jre-legacy")
        {
            8u8
        } else if name.contains("java-runtime-alpha") {
            16u8
        } else if name.contains("java-runtime-beta") || name.contains("java-runtime-gamma") {
            17u8
        } else if let Some(n) = name.split('-').nth(1).and_then(|s| s.parse::<u8>().ok()) {
            n
        } else {
            // Fallback: try to run java -version
            let java_bin = if cfg!(windows) {
                path.join("bin/java.exe")
            } else {
                path.join("bin/java")
            };
            let mut cmd = std::process::Command::new(&java_bin);
            cmd.arg("-version");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            if let Ok(output) = cmd.output() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("1.8.0") || stderr.contains("\"1.8") {
                    8u8
                } else if stderr.contains("version \"21") {
                    21u8
                } else if stderr.contains("version \"17") {
                    17u8
                } else if stderr.contains("version \"25") {
                    25u8
                } else {
                    0
                }
            } else {
                0
            }
        };
        candidates.push((major, path));
    }

    // If we have a required version, filter and sort
    if let Some(required) = required_major {
        let mut filtered = candidates.clone();
        if required == 8 {
            // Forge 1.12.2 and older *strictly* require Java 8
            filtered.retain(|(v, _)| *v == 8);
        } else {
            // Modern Minecraft (1.17+) is generally compatible with newer versions than requested
            filtered.retain(|(v, _)| *v >= required);
        }

        if !filtered.is_empty() {
            // Sort by closeness to required version
            filtered.sort_by_key(|(v, _)| (*v as i16 - required as i16).abs());
            if let Some((v, java_home)) = filtered.first() {
                let java_bin = if cfg!(windows) {
                    java_home.join("bin/java.exe")
                } else {
                    java_home.join("bin/java")
                };
                log::info!("Selected JVM: {:?} (detected major: {})", java_home, v);
                return Ok((*v, java_bin, Some(java_home.clone())));
            }
        }

        if required == 8 {
            return Err("Java 8 (1.8) is required but not found on your system. The launcher should have downloaded it automatically, please try re-syncing.".to_string());
        }
    } else {
        // No required version, just pick highest
        candidates.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
        if let Some((v, java_home)) = candidates.first() {
            let java_bin = if cfg!(windows) {
                java_home.join("bin/java.exe")
            } else {
                java_home.join("bin/java")
            };
            log::info!(
                "Selected JVM (Highest): {:?} (detected major: {})",
                java_home,
                v
            );
            return Ok((*v, java_bin, Some(java_home.clone())));
        }
    }

    // Fallback to system java only if no version is required or if we're desperate
    let java_bin = which("java")
        .map_err(|_| "Java binary not found in PATH. Please install Java.".to_string())?;

    // Check if the system default java matches requirements
    let mut detected_major = 0u8;
    let mut cmd = std::process::Command::new(&java_bin);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("Failed to check java version: {}", e))?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    if stderr.contains("1.8.0") || stderr.contains("\"1.8") {
        detected_major = 8;
    } else if stderr.contains("version \"17") {
        detected_major = 17;
    } else if stderr.contains("version \"21") {
        detected_major = 21;
    } else if stderr.contains("version \"25") {
        detected_major = 25;
    }

    if let Some(required) = required_major {
        if required == 8 && detected_major != 8 {
            return Err("Java 8 (1.8) is required but your system 'java' is a different version. Please install Java 8.".to_string());
        }
    }

    let java_home = java_bin
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    Ok((detected_major, java_bin, java_home))
}

impl LaunchArguments {
    pub fn resolve_assets_dir(session: &Session, official_mc_path: &Path) -> PathBuf {
        session
            .assets_path
            .as_deref()
            .map(|p| p.trim().trim_matches(['"', '\'']))
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| official_mc_path.join("assets"))
    }

    pub fn guarantee_assets_dir_and_subdirs(assets_dir: &Path) -> Result<(), String> {
        let clean = assets_dir.to_string_lossy();
        let clean_trimmed = clean.trim().trim_matches(['"', '\'']);
        if clean_trimmed.is_empty() {
            return Err("Assets directory path cannot be empty".to_string());
        }
        let target = Path::new(clean_trimmed);
        fs::create_dir_all(target)
            .map_err(|e| format!("Failed to create assets directory {:?}: {}", target, e))?;
        let _ = fs::create_dir_all(target.join("indexes"));
        let _ = fs::create_dir_all(target.join("objects"));
        Ok(())
    }

    pub fn ensure_assets_dir_exists(assets_dir: &Path) -> Result<(), String> {
        Self::guarantee_assets_dir_and_subdirs(assets_dir)
    }

    pub fn extract_assets_dir(minecraft_args: &[String]) -> Option<PathBuf> {
        let mut result = None;
        for (pos, a) in minecraft_args.iter().enumerate() {
            if a == "--assetsDir" {
                if let Some(p) = minecraft_args.get(pos + 1) {
                    let clean = p.trim().trim_matches(['"', '\'']);
                    if !clean.is_empty() && !clean.starts_with("--") {
                        result = Some(PathBuf::from(clean));
                    }
                }
            } else if let Some(stripped) = a.strip_prefix("--assetsDir=") {
                let clean = stripped.trim().trim_matches(['"', '\'']);
                if !clean.is_empty() {
                    result = Some(PathBuf::from(clean));
                }
            }
        }
        result
    }

    pub fn extract_asset_index_id(minecraft_args: &[String]) -> Option<String> {
        let mut result = None;
        for (pos, a) in minecraft_args.iter().enumerate() {
            if a == "--assetIndex" {
                if let Some(id) = minecraft_args.get(pos + 1) {
                    let clean = id.trim().trim_matches(['"', '\'']);
                    if !clean.starts_with("--") && !clean.is_empty() {
                        result = Some(clean.to_string());
                    }
                }
            } else if let Some(stripped) = a.strip_prefix("--assetIndex=") {
                let clean = stripped.trim().trim_matches(['"', '\'']);
                if !clean.is_empty() {
                    result = Some(clean.to_string());
                }
            }
        }
        result
    }

    pub fn from_session(
        session: &Session,
        session_dir: &Path,
        auth: &AuthResponse,
        settings: &crate::core::settings::AppSettings,
    ) -> Result<Self, String> {
        ensure_pointblank_effect_hotfix(session_dir);

        // Resolve the official .minecraft path
        let official_mc_path = if cfg!(windows) {
            PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft")
        } else {
            let home = std::env::var("HOME").map_err(|_| "Could not find HOME directory")?;
            PathBuf::from(home).join(".minecraft")
        };

        // Resolve assets directory respecting session.assets_path and guarantee its existence on disk
        let assets_dir = Self::resolve_assets_dir(session, &official_mc_path);
        Self::ensure_assets_dir_exists(&assets_dir)?;
        let _ = fs::create_dir_all(assets_dir.join("indexes"));
        let _ = fs::create_dir_all(assets_dir.join("objects"));

        let version_id_owned = if let Some(forge_version) = &session.forge {
            format!("{}-forge-{}", session.minecraft, forge_version)
        } else if let Some(fabric_version) = &session.fabric {
            format!("fabric-loader-{}-{}", fabric_version, session.minecraft)
        } else if let Some(quilt_version) = &session.quilt {
            format!("quilt-loader-{}-{}", quilt_version, session.minecraft)
        } else if let Some(neoforge_version) = &session.neoforge {
            format!("neoforge-{}", neoforge_version)
        } else {
            session.minecraft.clone()
        };
        let version_id = &version_id_owned;

        let version_json_path = official_mc_path
            .join("versions")
            .join(version_id)
            .join(format!("{}.json", version_id));

        if !version_json_path.exists() {
            return Err(format!(
                "Version JSON not found at {:?}. Please ensure the version is installed.",
                version_json_path
            ));
        }

        let content = fs::read_to_string(&version_json_path)
            .map_err(|e| format!("Failed to read version JSON: {}", e))?;

        let mut manifest: VersionManifest = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse version JSON: {}", e))?;

        if let Some(parent_id) = &manifest.inherits_from {
            let parent_json_path = official_mc_path
                .join("versions")
                .join(parent_id)
                .join(format!("{}.json", parent_id));
            if parent_json_path.exists() {
                if let Ok(parent_content) = fs::read_to_string(&parent_json_path) {
                    if let Ok(parent_manifest) =
                        serde_json::from_str::<VersionManifest>(&parent_content)
                    {
                        let child_needs_asset_index = manifest
                            .asset_index
                            .as_ref()
                            .map_or(true, |a| a.url.as_deref().unwrap_or("").trim().is_empty());
                        if child_needs_asset_index && parent_manifest.asset_index.is_some() {
                            manifest.asset_index = parent_manifest.asset_index;
                        }
                        if manifest.java_version.is_none() {
                            manifest.java_version = parent_manifest.java_version;
                        }
                        if manifest.minecraft_arguments.is_none() {
                            manifest.minecraft_arguments = parent_manifest.minecraft_arguments;
                        }

                        if let Some(parent_args) = parent_manifest.arguments {
                            if let Some(ref mut child_args) = manifest.arguments {
                                let mut merged_game = parent_args.game;
                                merged_game.extend(child_args.game.clone());
                                child_args.game = merged_game;

                                let mut merged_jvm = parent_args.jvm;
                                merged_jvm.extend(child_args.jvm.clone());
                                child_args.jvm = merged_jvm;
                            } else {
                                manifest.arguments = Some(parent_args);
                            }
                        }

                        let mut merged_from_parent = 0usize;
                        for parent_lib in parent_manifest.libraries {
                            if !manifest.libraries.iter().any(|l| l.name == parent_lib.name) {
                                manifest.libraries.push(parent_lib);
                                merged_from_parent += 1;
                            }
                        }
                        log::info!(
                            "Merged {} libraries and arguments from parent {}",
                            merged_from_parent,
                            parent_id
                        );
                    }
                }
            }
        }

        let os = current_os_name();
        let lib_base = official_mc_path.join("libraries");

        // Create natives extraction directory
        let natives_dir = official_mc_path
            .join("versions")
            .join(version_id)
            .join(format!("{}-natives", version_id));
        fs::create_dir_all(&natives_dir)
            .map_err(|e| format!("Failed to create natives dir: {}", e))?;

        let mut classpath = Vec::new();
        let mut classpath_seen: HashSet<PathBuf> = HashSet::new();
        let mut missing_libs = Vec::new();

        // 1. Process libraries
        for lib in &manifest.libraries {
            if !library_allowed(&lib.rules) {
                continue;
            }

            if let Some(downloads) = &lib.downloads {
                // Main artifact → classpath
                if let Some(artifact) = &downloads.artifact {
                    let p = lib_base.join(&artifact.path);
                    if p.exists() {
                        if classpath_seen.insert(p.clone()) {
                            classpath.push(p);
                        }
                    } else {
                        missing_libs.push(artifact.path.clone());
                    }
                }

                // Natives → extract to natives_dir (do NOT add to classpath)
                if let (Some(natives_map), Some(classifiers)) =
                    (&lib.natives, &downloads.classifiers)
                {
                    if let Some(native_key) = natives_map.get(os) {
                        if let Some(native_art) = classifiers.get(native_key) {
                            let native_jar = lib_base.join(&native_art.path);
                            if native_jar.exists() {
                                if let Err(e) = extract_natives(&native_jar, &natives_dir) {
                                    log::warn!(
                                        "Failed to extract natives from {:?}: {}",
                                        native_jar,
                                        e
                                    );
                                }
                            } else {
                                missing_libs.push(native_art.path.clone());
                            }
                        }
                    }
                }
            } else {
                // Fallback: reconstruct path from maven coordinates
                let parts: Vec<&str> = lib.name.split(':').collect();
                if parts.len() >= 3 {
                    let group = parts[0].replace('.', "/");
                    let artifact_name = parts[1];
                    let ver = parts[2];
                    let jar_name = format!("{}-{}.jar", artifact_name, ver);
                    let p = lib_base
                        .join(&group)
                        .join(artifact_name)
                        .join(ver)
                        .join(&jar_name);
                    if p.exists() && classpath_seen.insert(p.clone()) {
                        classpath.push(p);
                    }
                }
            }
        }

        if !missing_libs.is_empty() {
            log::warn!(
                "{} libraries missing from disk (re-sync to download): {:?}",
                missing_libs.len(),
                &missing_libs[..missing_libs.len().min(3)]
            );
        }
        log::info!(
            "Classpath: {} libraries. Natives dir: {:?}",
            classpath.len(),
            natives_dir
        );

        // 2. Add vanilla client JAR only when it is truly needed.
        // Modern Forge (bootstraplauncher) resolves Minecraft client modules itself; adding
        // versions/<mc>/<mc>.jar here creates module conflicts (minecraft vs _1._20._1).
        let is_modern_forge_bootstrap = session.forge.is_some()
            && manifest
                .main_class
                .contains("cpw.mods.bootstraplauncher.BootstrapLauncher");

        let has_forge_minecraft_client = manifest
            .libraries
            .iter()
            .any(|lib| lib.name.starts_with("net.minecraft:client:"));

        if !has_forge_minecraft_client && !is_modern_forge_bootstrap {
            let root_jar_id = manifest
                .jar
                .as_deref()
                .or(manifest.inherits_from.as_deref())
                .unwrap_or(version_id);
            let client_jar_path = official_mc_path
                .join("versions")
                .join(root_jar_id)
                .join(format!("{}.jar", root_jar_id));

            if client_jar_path.exists() {
                if classpath_seen.insert(client_jar_path.clone()) {
                    classpath.push(client_jar_path);
                }
            } else {
                return Err(format!("Client JAR not found at {:?}", client_jar_path));
            }
        } else {
            log::info!(
                "Skipping root client JAR for {}: handled by Forge/module bootstrap or forge client libraries",
                version_id
            );
        }

        // Get session-specific settings or fallback to default
        let session_settings = settings.sessions.get(&session.name).unwrap_or(&settings.default_settings);

        // JVM args
        let mut jvm_args = Vec::new();

        // CRITICAL: tell JVM where to find native .so/.dll files
        jvm_args.push(format!("-Djava.library.path={}", natives_dir.display()));

        let jvm_placeholders = [
            (
                "${natives_directory}",
                natives_dir.to_string_lossy().to_string(),
            ),
            (
                "${library_directory}",
                official_mc_path
                    .join("libraries")
                    .to_string_lossy()
                    .to_string(),
            ),
            ("${classpath_separator}", if cfg!(windows) { ";" } else { ":" }.to_string()),
            ("${launcher_name}", "launched".to_string()),
            ("${launcher_version}", env!("CARGO_PKG_VERSION").to_string()),
        ];

        // Include launcher-provided JVM args from the manifest (modern format).
        if let Some(args) = &manifest.arguments {
            for raw in &args.jvm {
                for s in argument_value_strings(raw) {
                    let resolved = apply_placeholders(&s, &jvm_placeholders);

                    // Keep control of classpath and memory flags in the launcher.
                    if resolved == "-cp"
                        || resolved == "--class-path"
                        || resolved.contains("${classpath}")
                        || resolved.starts_with("-Xmx")
                        || resolved.starts_with("-Xms")
                    {
                        continue;
                    }

                    jvm_args.push(resolved);
                }
            }
        }

        // Memory args from settings
        jvm_args.push(format!("-Xms{}M", session_settings.min_ram));
        jvm_args.push(format!("-Xmx{}M", session_settings.max_ram));

        // Function to filter out conflicting memory args from custom strings
        let filter_mem_args = |s: &str| -> Vec<String> {
            s.split_whitespace()
                .filter(|arg| !arg.starts_with("-Xmx") && !arg.starts_with("-Xms"))
                .map(|s| s.to_string())
                .collect()
        };

        // Use custom JVM args from session settings if provided, else fallback to session defaults
        if !session_settings.jvm_args.is_empty() {
            jvm_args.extend(filter_mem_args(&session_settings.jvm_args));
        } else if !session.jvm_arg.is_empty() {
            jvm_args.extend(filter_mem_args(&session.jvm_arg));
        }

        // Minecraft game args
        let mut minecraft_args = Vec::new();

        let is_offline = auth.access_token == "offline";
        let user_type = if is_offline { "mojang".to_string() } else { "msa".to_string() };
        let xuid = if is_offline { "".to_string() } else { auth.uuid.replace('-', "") };

        if let Some(args) = &manifest.arguments {
            let game_placeholders = [
                ("${auth_player_name}", auth.name.clone()),
                ("${version_name}", version_id.clone()),
                (
                    "${game_directory}",
                    session_dir.to_string_lossy().to_string(),
                ),
                (
                    "${assets_root}",
                    assets_dir
                        .to_string_lossy()
                        .to_string(),
                ),
                (
                    "${game_assets}",
                    assets_dir
                        .to_string_lossy()
                        .to_string(),
                ),
                (
                    "${assets_index_name}",
                    manifest
                        .asset_index
                        .as_ref()
                        .map(|a| a.id.clone())
                        .unwrap_or_else(|| {
                            if !session.minecraft.is_empty() {
                                session.minecraft.clone()
                            } else {
                                "1.12".to_string()
                            }
                        }),
                ),
                ("${auth_uuid}", auth.uuid.clone()),
                ("${auth_access_token}", auth.access_token.clone()),
                ("${auth_session}", auth.access_token.clone()),
                ("${user_type}", user_type.clone()),
                ("${version_type}", "release".to_string()),
                ("${clientid}", "00000000402b5328".to_string()),
                ("${auth_xuid}", xuid.clone()),
                ("${user_properties}", "{}".to_string()),
                (
                    "${resolution_width}",
                    settings
                        .game_resolution
                        .split('x')
                        .next()
                        .unwrap_or("854")
                        .to_string(),
                ),
                (
                    "${resolution_height}",
                    settings
                        .game_resolution
                        .split('x')
                        .nth(1)
                        .unwrap_or("480")
                        .to_string(),
                ),
            ];

            for raw in &args.game {
                for s in argument_value_strings(raw) {
                    minecraft_args.push(apply_placeholders(&s, &game_placeholders));
                }
            }
        }

        if minecraft_args.is_empty() {
            if let Some(arg_line) = manifest.minecraft_arguments {
                // Old format: single string with placeholders
                let map = [
                    ("${auth_player_name}", auth.name.clone()),
                    ("${version_name}", version_id.clone()),
                    (
                        "${game_directory}",
                        session_dir.to_string_lossy().to_string(),
                    ),
                    (
                        "${assets_root}",
                        assets_dir
                            .to_string_lossy()
                            .to_string(),
                    ),
                    (
                        "${game_assets}",
                        assets_dir
                            .to_string_lossy()
                            .to_string(),
                    ),
                    (
                        "${assets_index_name}",
                        manifest
                            .asset_index
                            .as_ref()
                            .map(|a| a.id.clone())
                            .unwrap_or_else(|| {
                                if !session.minecraft.is_empty() {
                                    session.minecraft.clone()
                                } else {
                                    "1.12".to_string()
                                }
                            }),
                    ),
                    ("${auth_uuid}", auth.uuid.clone()),
                    ("${auth_access_token}", auth.access_token.clone()),
                    ("${auth_session}", auth.access_token.clone()),
                    ("${user_type}", user_type.clone()),
                    ("${version_type}", "release".to_string()),
                    ("${clientid}", "00000000402b5328".to_string()),
                    ("${auth_xuid}", xuid.clone()),
                    ("${user_properties}", "{}".to_string()),
                    ("${resolution_width}", settings.game_resolution.split('x').next().unwrap_or("854").to_string()),
                    ("${resolution_height}", settings.game_resolution.split('x').nth(1).unwrap_or("480").to_string()),
                ];

                for raw_token in arg_line.split_whitespace() {
                    let mut token = raw_token.to_string();
                    for (placeholder, value) in &map {
                        token = token.replace(placeholder, value);
                    }
                    if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
                        token = token[1..token.len() - 1].to_string();
                    }
                    minecraft_args.push(token);
                }
            } else {
                // Modern format or fallback
                minecraft_args.push("--username".to_string());
                minecraft_args.push(auth.name.clone());
                minecraft_args.push("--version".to_string());
                minecraft_args.push(version_id.clone());
                minecraft_args.push("--gameDir".to_string());
                minecraft_args.push(session_dir.to_string_lossy().to_string());
                minecraft_args.push("--assetsDir".to_string());
                minecraft_args.push(
                    assets_dir
                        .to_string_lossy()
                        .to_string(),
                );
                minecraft_args.push("--assetIndex".to_string());
                minecraft_args.push(
                    manifest
                        .asset_index
                        .as_ref()
                        .map(|a| a.id.clone())
                        .unwrap_or_else(|| {
                            if !session.minecraft.is_empty() {
                                session.minecraft.clone()
                            } else {
                                "1.12".to_string()
                            }
                        }),
                );
                minecraft_args.push("--uuid".to_string());
                minecraft_args.push(auth.uuid.clone());
                minecraft_args.push("--accessToken".to_string());
                minecraft_args.push(auth.access_token.clone());
                minecraft_args.push("--userType".to_string());
                minecraft_args.push(user_type.clone());
                minecraft_args.push("--versionType".to_string());
                minecraft_args.push("release".to_string());
                if !xuid.is_empty() {
                    minecraft_args.push("--xuid".to_string());
                    minecraft_args.push(xuid.clone());
                    minecraft_args.push("--clientId".to_string());
                    minecraft_args.push("00000000402b5328".to_string());
                }
                
                // Add resolution
                if let Some((w, h)) = settings.game_resolution.split_once('x') {
                    minecraft_args.push("--width".to_string());
                    minecraft_args.push(w.to_string());
                    minecraft_args.push("--height".to_string());
                    minecraft_args.push(h.to_string());
                }
            }
        }

        // Some loader profiles provide partial game arguments; guarantee mandatory options.
        if !minecraft_args.iter().any(|a| a == "--username") {
            minecraft_args.push("--username".to_string());
            minecraft_args.push(auth.name.clone());
        }
        if !minecraft_args.iter().any(|a| a == "--version") {
            minecraft_args.push("--version".to_string());
            minecraft_args.push(version_id.clone());
        }
        if !minecraft_args.iter().any(|a| a == "--gameDir") {
            minecraft_args.push("--gameDir".to_string());
            minecraft_args.push(session_dir.to_string_lossy().to_string());
        }
        if !minecraft_args.iter().any(|a| a == "--assetsDir" || a.starts_with("--assetsDir=")) {
            minecraft_args.push("--assetsDir".to_string());
            minecraft_args.push(
                assets_dir
                    .to_string_lossy()
                    .to_string(),
            );
        }
        if !minecraft_args.iter().any(|a| a == "--assetIndex" || a.starts_with("--assetIndex=")) {
            let default_index = if !session.minecraft.is_empty() {
                session.minecraft.clone()
            } else {
                "1.12".to_string()
            };
            minecraft_args.push("--assetIndex".to_string());
            minecraft_args.push(
                manifest
                    .asset_index
                    .as_ref()
                    .map(|a| a.id.clone())
                    .unwrap_or(default_index),
            );
        }
        if !minecraft_args.iter().any(|a| a == "--uuid") {
            minecraft_args.push("--uuid".to_string());
            minecraft_args.push(auth.uuid.clone());
        }
        if !minecraft_args.iter().any(|a| a == "--accessToken") {
            minecraft_args.push("--accessToken".to_string());
            minecraft_args.push(auth.access_token.clone());
        }
        if !minecraft_args.iter().any(|a| a == "--userType") {
            minecraft_args.push("--userType".to_string());
            minecraft_args.push(user_type);
        }
        if !minecraft_args.iter().any(|a| a == "--versionType") {
            minecraft_args.push("--versionType".to_string());
            minecraft_args.push("release".to_string());
        }

        // Guarantee that demo mode and quickPlay artifacts are completely removed
        minecraft_args.retain(|a| {
            a != "--demo"
                && !a.starts_with("--quickPlay")
                && !a.starts_with("${quickPlay")
        });

        // Guarantee that the directory path passed to --assetsDir exists on disk (including subdirs) prior to process execution
        let effective_assets_dir = Self::extract_assets_dir(&minecraft_args).unwrap_or_else(|| assets_dir.clone());
        Self::guarantee_assets_dir_and_subdirs(&effective_assets_dir)?;

        let default_index_fallback = if !session.minecraft.is_empty() {
            session.minecraft.clone()
        } else {
            "1.12".to_string()
        };

        // Ensure asset index file is downloaded or created if missing.
        // Priority: CLI argument value (what Minecraft will actually open) -> manifest assetIndex -> session.minecraft fallback
        let resolved_index_id = Self::extract_asset_index_id(&minecraft_args)
            .or_else(|| manifest.asset_index.as_ref().map(|a| a.id.clone()))
            .unwrap_or(default_index_fallback);

        let index_path = effective_assets_dir
            .join("indexes")
            .join(format!("{}.json", resolved_index_id));

        let is_corrupted_or_missing = !index_path.exists()
            || fs::metadata(&index_path).map(|m| m.len() == 0).unwrap_or(true)
            || match fs::read_to_string(&index_path) {
                Ok(content) => match serde_json::from_str::<Value>(&content) {
                    Ok(val) => {
                        if manifest.asset_index.as_ref().and_then(|a| a.url.as_deref()).map_or(false, |u| !u.trim().is_empty()) {
                            val.get("objects").and_then(|o| o.as_object()).map_or(true, |o| o.is_empty())
                        } else {
                            false
                        }
                    }
                    Err(_) => true,
                },
                Err(_) => true,
            };

        if is_corrupted_or_missing {
            if let Some(parent) = index_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            let mut downloaded = false;
            if let Some(ref asset_index) = manifest.asset_index {
                if let Some(ref url) = asset_index.url {
                    let trimmed_url = url.trim();
                    if !trimmed_url.is_empty() {
                        if let Ok(resp) = ureq::get(trimmed_url)
                            .timeout(std::time::Duration::from_secs(10))
                            .call()
                        {
                            if resp.status() == 200 {
                                if let Ok(text) = resp.into_string() {
                                    if !text.trim().is_empty()
                                        && serde_json::from_str::<Value>(&text).is_ok()
                                    {
                                        if let Ok(_) = fs::write(&index_path, &text) {
                                            downloaded = true;
                                            let manifest_index_path = effective_assets_dir
                                                .join("indexes")
                                                .join(format!("{}.json", asset_index.id));
                                            if manifest_index_path != index_path && !manifest_index_path.exists() {
                                                let _ = fs::write(&manifest_index_path, &text);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Fallback: If index could not be downloaded and does not exist on disk, create minimal index
            if !downloaded
                && (!index_path.exists()
                    || fs::metadata(&index_path).map(|m| m.len() == 0).unwrap_or(true)
                    || fs::read_to_string(&index_path)
                        .map(|s| serde_json::from_str::<Value>(&s).is_err())
                        .unwrap_or(true))
            {
                let _ = fs::write(&index_path, r#"{"objects":{}}"#);
            }
        }

        // Find correct Java version (MC 1.12.2 needs Java 8)
        let required_java = manifest
            .java_version
            .as_ref()
            .map(|j| j.major_version)
            .or(Some(8));
        let (java_major, java_path, java_home) = find_java(required_java, &official_mc_path)?;
        log::info!("Using java: {:?}, home: {:?}", java_path, java_home);

        // Allow native access (suppresses LWJGL warning on Java 17+)
        if java_major >= 17 {
            jvm_args.push("--enable-native-access=ALL-UNNAMED".to_string());

            // Forge bootstrap libraries may reflectively access MethodHandles internals on modern JVMs.
            if session.forge.is_some() {
                jvm_args.push("--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string());
                jvm_args.push("--add-opens=java.base/java.util.jar=ALL-UNNAMED".to_string());
            }
        }

        Ok(LaunchArguments {
            java_path,
            java_home,
            game_dir: session_dir.to_path_buf(),
            jvm_args,
            classpath,
            main_class: manifest.main_class,
            minecraft_args,
            wrapper_command: session_settings.wrapper_command.clone(),
        })
    }

    pub fn build(&self) -> Vec<String> {
        let mut args = self.jvm_args.clone();

        // Add classpath
        args.push("-cp".to_string());
        let cp = self
            .classpath
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<String>>()
            .join(if cfg!(windows) { ";" } else { ":" });
        args.push(cp);

        // Add main class
        args.push(self.main_class.clone());

        // Add Minecraft args
        args.extend(self.minecraft_args.clone());

        args
    }
}
