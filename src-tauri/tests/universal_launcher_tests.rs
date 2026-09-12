use app_lib::core::install::assets::AssetManager;
use app_lib::core::install::mojang::VersionDetail;
use app_lib::core::install::runtime::JreManager;
use app_lib::core::launch::args::LaunchArguments;
use app_lib::core::launch::models::VersionManifest;
use app_lib::core::meta::models::*;
use app_lib::core::meta::prism::PrismMetaClient;
use app_lib::core::session::{ComponentSpec, Session};
use std::path::PathBuf;


#[test]
fn test_maven_coordinate_resolution() {
    assert_eq!(
        PrismMetaClient::maven_coordinate_to_path("org.ow2.asm:asm:9.5"),
        Some("org/ow2/asm/asm/9.5/asm-9.5.jar".to_string())
    );

    assert_eq!(
        PrismMetaClient::maven_coordinate_to_path("net.minecraftforge:forge:1.20.1-47.2.0:universal"),
        Some("net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-universal.jar".to_string())
    );

    assert_eq!(
        PrismMetaClient::maven_coordinate_to_path("org.lwjgl:lwjgl-jemalloc:3.3.1:natives-windows"),
        Some("org/lwjgl/lwjgl-jemalloc/3.3.1/lwjgl-jemalloc-3.3.1-natives-windows.jar".to_string())
    );

    assert_eq!(
        PrismMetaClient::maven_coordinate_to_path("invalid_coord"),
        None
    );
}

#[test]
fn test_session_effective_components_legacy_fallback() {
    let session_forge = Session {
        name: "Forge Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: Some("47.2.0".to_string()),
        neoforge: None,
        fabric: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: None,
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let comps = session_forge.effective_components();
    assert_eq!(comps.len(), 2);
    assert_eq!(comps[0], ComponentSpec { uid: "net.minecraft".to_string(), version: "1.20.1".to_string() });
    assert_eq!(comps[1], ComponentSpec { uid: "net.minecraftforge".to_string(), version: "47.2.0".to_string() });

    let session_neoforge = Session {
        name: "NeoForge Session".to_string(),
        minecraft: "1.20.4".to_string(),
        forge: None,
        neoforge: Some("20.4.80".to_string()),
        fabric: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: None,
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let comps_neo = session_neoforge.effective_components();
    assert_eq!(comps_neo.len(), 2);
    assert_eq!(comps_neo[0], ComponentSpec { uid: "net.minecraft".to_string(), version: "1.20.4".to_string() });
    assert_eq!(comps_neo[1], ComponentSpec { uid: "net.neoforged".to_string(), version: "20.4.80".to_string() });
}

#[test]
fn test_session_custom_components_priority() {
    let custom_comps = vec![
        ComponentSpec { uid: "net.minecraft".to_string(), version: "1.20.1".to_string() },
        ComponentSpec { uid: "net.fabricmc.fabric-loader".to_string(), version: "0.15.11".to_string() },
    ];

    let session = Session {
        name: "Custom Components".to_string(),
        minecraft: "1.12.2".to_string(),
        forge: Some("14.23.5.2860".to_string()),
        neoforge: None,
        fabric: None,
        quilt: None,
        components: Some(custom_comps.clone()),
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: None,
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    assert_eq!(session.effective_components(), custom_comps);
}

#[test]
fn test_jre_resolution() {
    assert_eq!(JreManager::resolve_component_name(None, Some(21), None), "java-runtime-delta");
    assert_eq!(JreManager::resolve_component_name(None, Some(17), None), "java-runtime-gamma");
    assert_eq!(JreManager::resolve_component_name(None, Some(16), None), "java-runtime-alpha");
    assert_eq!(JreManager::resolve_component_name(None, Some(8), None), "jre-legacy");
    assert_eq!(JreManager::resolve_component_name(None, None, None), "jre-legacy");
    assert_eq!(JreManager::resolve_component_name(None, None, Some("java-runtime-delta")), "java-runtime-delta");

    // Version deductions
    assert_eq!(JreManager::resolve_component_name(Some("1.20.1"), None, None), "java-runtime-gamma"); // Java 17
    assert_eq!(JreManager::resolve_component_name(Some("1.20.6"), None, None), "java-runtime-delta"); // Java 21
    assert_eq!(JreManager::resolve_component_name(Some("1.21.1"), None, None), "java-runtime-delta"); // Java 21
    assert_eq!(JreManager::resolve_component_name(Some("1.17.1"), None, None), "java-runtime-alpha"); // Java 16
    assert_eq!(JreManager::resolve_component_name(Some("1.12.2"), None, None), "jre-legacy");        // Java 8
    assert_eq!(JreManager::resolve_component_name(Some("1.7.10"), None, None), "jre-legacy");        // Java 8
}

#[test]
fn test_library_rule_filtering() {
    let current_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };

    let allowed_rules = Some(vec![LibraryRule {
        action: "allow".to_string(),
        os: Some(OsCondition {
            name: Some(current_os.to_string()),
            version: None,
            arch: None,
        }),
        features: None,
    }]);

    assert!(PrismMetaClient::is_library_allowed(&allowed_rules));

    let disallowed_rules = Some(vec![
        LibraryRule {
            action: "allow".to_string(),
            os: None,
            features: None,
        },
        LibraryRule {
            action: "disallow".to_string(),
            os: Some(OsCondition {
                name: Some(current_os.to_string()),
                version: None,
                arch: None,
            }),
            features: None,
        },
    ]);

    assert!(!PrismMetaClient::is_library_allowed(&disallowed_rules));
}

#[tokio::test]
async fn test_fetch_real_component_manifest() {
    let client = PrismMetaClient::new(std::env::temp_dir().join("test_mc"));
    let manifest = client.get_component("net.minecraft", "1.12.2").await;
    assert!(manifest.is_ok(), "Failed to fetch 1.12.2 manifest: {:?}", manifest.err());
    let _m = manifest.unwrap();
    let m1201 = client.get_component("net.minecraft", "1.20.1").await.unwrap();
    println!("1.20.1 jvm arguments: {:?}", m1201.arguments.as_ref().map(|a| &a.jvm));
    println!("1.20.1 plus_jvm_args: {:?}", m1201.plus_jvm_args);
    let forge1201 = client.get_component("net.minecraftforge", "47.4.0").await.unwrap();
    println!("Forge 47.4.0 jvm arguments: {:?}", forge1201.arguments.as_ref().map(|a| &a.jvm));
    println!("Forge 47.4.0 plus_jvm_args: {:?}", forge1201.plus_jvm_args);
}

#[tokio::test]
async fn test_fetch_all_servers_json_versions() {
    let client = PrismMetaClient::new(std::env::temp_dir().join("test_mc"));

    // 1.12.2 Forge 14.23.5.2847
    let v2847 = client.get_component("net.minecraftforge", "14.23.5.2847").await;
    println!("Forge 14.23.5.2847 result: {:?}", v2847.as_ref().map(|m| &m.main_class));
    assert!(v2847.is_ok(), "Failed 14.23.5.2847: {:?}", v2847.err());

    // 1.12.2 Forge 14.23.5.2859
    let v2859 = client.get_component("net.minecraftforge", "14.23.5.2859").await;
    println!("Forge 14.23.5.2859 result: {:?}", v2859.as_ref().map(|m| &m.main_class));
    assert!(v2859.is_ok(), "Failed 14.23.5.2859: {:?}", v2859.err());

    // 1.20.1 Forge 47.4.0
    let v4740 = client.get_component("net.minecraftforge", "47.4.0").await;
    assert!(v4740.is_ok(), "Failed 47.4.0: {:?}", v4740.err());
    let m4740 = v4740.unwrap();
    assert_eq!(m4740.main_class.as_deref(), Some("io.github.zekerzhayard.forgewrapper.installer.Main"));
    assert!(m4740.maven_files.as_ref().unwrap().iter().any(|f| f.name.contains(":installer")), "Installer jar must be in mavenFiles");

    if let Some(libs) = &m4740.libraries {
        for l in libs {
            println!("Forge 47.4.0 lib: {} -> MMC-hint: {:?}", l.name, l.mmc_hint);
        }
    }

    let fab = client.get_component("net.fabricmc.fabric-loader", "0.15.11").await.unwrap();
    println!("Fabric Loader 0.15.11 requires: {:?}", fab.requires);
}

#[tokio::test]
async fn test_resolve_fabric_components() {
    let client = PrismMetaClient::new(std::env::temp_dir().join("test_mc"));
    let initial = vec![
        ComponentSpec { uid: "net.minecraft".to_string(), version: "1.20.1".to_string() },
        ComponentSpec { uid: "net.fabricmc.fabric-loader".to_string(), version: "0.15.11".to_string() },
    ];

    let manifests = client.resolve_all_components(&initial).await.unwrap();
    println!("Resolved {} manifests for 1.20.1 Fabric:", manifests.len());
    for m in &manifests {
        println!(" - Component: {:?}", m.uid);
    }

    assert!(manifests.iter().any(|m| m.uid.as_deref() == Some("net.fabricmc.intermediary")), "net.fabricmc.intermediary must be resolved");
    let intermediary = manifests.iter().find(|m| m.uid.as_deref() == Some("net.fabricmc.intermediary")).unwrap();
    assert_eq!(intermediary.version.as_deref(), Some("1.20.1"), "Intermediary must use Minecraft version 1.20.1");
}

#[tokio::test]
async fn test_resolve_all_components_includes_lwjgl() {
    let client = PrismMetaClient::new(std::env::temp_dir().join("test_mc"));
    let initial = vec![
        ComponentSpec { uid: "net.minecraft".to_string(), version: "1.12.2".to_string() },
        ComponentSpec { uid: "net.minecraftforge".to_string(), version: "14.23.5.2847".to_string() },
    ];

    let manifests = client.resolve_all_components(&initial).await.unwrap();
    println!("Resolved {} manifests for 1.12.2 Forge:", manifests.len());
    for m in &manifests {
        println!(" - Component: {:?}", m.uid);
    }

    assert!(manifests.iter().any(|m| m.uid.as_deref() == Some("org.lwjgl")), "org.lwjgl must be resolved for 1.12.2");
    assert!(manifests.iter().any(|m| m.uid.as_deref() == Some("net.minecraft")), "net.minecraft must be resolved");
    assert!(manifests.iter().any(|m| m.uid.as_deref() == Some("net.minecraftforge")), "net.minecraftforge must be resolved");

    let lwjgl_m = manifests.iter().find(|m| m.uid.as_deref() == Some("org.lwjgl")).unwrap();
    println!("org.lwjgl libraries count: {:?}", lwjgl_m.libraries.as_ref().map(|l| l.len()));
    for lib in lwjgl_m.libraries.as_ref().unwrap() {
        println!(" - lib: {} natives: {:?} downloads: {:?}", lib.name, lib.natives, lib.downloads.is_some());
        let urls = PrismMetaClient::resolve_library_urls(lib);
        println!("   resolved urls: {:?}", urls);
    }
}

#[test]
fn test_crack_pseudo_validation() {
    fn validate_pseudo(pseudo: &str) -> bool {
        let trimmed = pseudo.trim();
        !trimmed.is_empty()
            && trimmed.len() <= 16
            && trimmed == pseudo
            && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    assert!(validate_pseudo("Infuseting"));
    assert!(validate_pseudo("Player_123"));
    assert!(validate_pseudo("a"));
    assert!(validate_pseudo("1234567890123456")); // 16 chars

    // Invalid cases
    assert!(!validate_pseudo(""));
    assert!(!validate_pseudo("   "));
    assert!(!validate_pseudo("TooLongUsernameExceedingLimit"));
    assert!(!validate_pseudo("Player with space"));
    assert!(!validate_pseudo("Steve;rm -rf /"));
    assert!(!validate_pseudo("Player\n"));
    assert!(!validate_pseudo("Player\0"));
}

#[test]
fn test_sync_path_traversal_sanitization() {
    fn is_safe_sync_path(path_str: &str) -> bool {
        let normalized = path_str.replace('\\', "/");
        !normalized.contains("..")
            && !normalized.starts_with('/')
            && !normalized.contains(':')
            && !normalized.is_empty()
    }

    assert!(is_safe_sync_path("mods/jei_1.12.2.jar"));
    assert!(is_safe_sync_path("config/custom.cfg"));
    assert!(is_safe_sync_path("resourcepacks/pack.zip"));

    // Traversal attacks
    assert!(!is_safe_sync_path("../../../Windows/System32/calc.exe"));
    assert!(!is_safe_sync_path("mods/../../escape.txt"));
    assert!(!is_safe_sync_path("/etc/passwd"));
    assert!(!is_safe_sync_path("C:/Users/danger.dll"));
}

#[test]
fn test_minecraft_arguments_spaces_preservation() {
    let raw_args = "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory}";
    let game_placeholders = [
        ("${auth_player_name}", "Infuseting".to_string()),
        ("${version_name}", "1.20.1".to_string()),
        ("${game_directory}", "C:\\Sessions\\Stolbovo RP".to_string()),
    ];

    let mut args = Vec::new();
    for raw_token in raw_args.split_whitespace() {
        let mut token = raw_token.to_string();
        for (placeholder, val) in &game_placeholders {
            token = token.replace(placeholder, val);
        }
        args.push(token);
    }

    assert_eq!(args.len(), 6);
    assert_eq!(args[4], "--gameDir");
    assert_eq!(args[5], "C:\\Sessions\\Stolbovo RP"); // Preserved whole!
}

#[test]
fn test_assets_directory_pre_launch_guarantee() {
    let test_dir = std::env::temp_dir().join(format!("test_assets_guarantee_{}", uuid::Uuid::new_v4().simple()));
    let nonexistent_assets = test_dir.join("missing_parent").join("assets");

    assert!(!nonexistent_assets.exists(), "Target assets dir must not exist initially");

    // Call ensure_assets_dir_exists
    let res = LaunchArguments::ensure_assets_dir_exists(&nonexistent_assets);
    assert!(res.is_ok(), "ensure_assets_dir_exists should succeed: {:?}", res.err());
    assert!(nonexistent_assets.exists(), "Target assets dir must exist on disk after guarantee call");

    // Call again on already-existing directory
    let res_again = LaunchArguments::ensure_assets_dir_exists(&nonexistent_assets);
    assert!(res_again.is_ok(), "ensure_assets_dir_exists on existing dir should succeed");

    // Clean up
    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_session_assets_path_customization_and_fallback() {
    let official_mc_path = PathBuf::from("/mock/minecraft");

    // 1. Explicit custom assets path
    let session_custom = Session {
        name: "Custom Assets Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: None,
        fabric: None,
        neoforge: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: Some("/custom/assets/directory".to_string()),
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let resolved_custom = LaunchArguments::resolve_assets_dir(&session_custom, &official_mc_path);
    assert_eq!(resolved_custom, PathBuf::from("/custom/assets/directory"));

    // 2. None -> fallback to official .minecraft/assets
    let session_default = Session {
        name: "Default Assets Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: None,
        fabric: None,
        neoforge: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: None,
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let resolved_default = LaunchArguments::resolve_assets_dir(&session_default, &official_mc_path);
    assert_eq!(resolved_default, official_mc_path.join("assets"));

    // 3. Whitespace string -> fallback to official .minecraft/assets
    let session_whitespace = Session {
        name: "Whitespace Assets Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: None,
        fabric: None,
        neoforge: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: Some("   ".to_string()),
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let resolved_ws = LaunchArguments::resolve_assets_dir(&session_whitespace, &official_mc_path);
    assert_eq!(resolved_ws, official_mc_path.join("assets"));

    // 4. Whitespace-padded custom assets path -> trimmed cleanly
    let session_padded = Session {
        name: "Padded Assets Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: None,
        fabric: None,
        neoforge: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: Some("   /custom/assets/directory   ".to_string()),
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let resolved_padded = LaunchArguments::resolve_assets_dir(&session_padded, &official_mc_path);
    assert_eq!(resolved_padded, PathBuf::from("/custom/assets/directory"));

    // 5. Quoted custom assets path -> quotes stripped
    let session_quoted = Session {
        name: "Quoted Assets Session".to_string(),
        minecraft: "1.20.1".to_string(),
        forge: None,
        fabric: None,
        neoforge: None,
        quilt: None,
        components: None,
        sync_dir: "mods".to_string(),
        sync_url: "https://example.com/sync".to_string(),
        welcome: "Welcome".to_string(),
        jvm_arg: "".to_string(),
        credits: "".to_string(),
        assets_path: Some("\"/custom/assets/directory\"".to_string()),
        hostname: None,
        is_default: false,
        links: None,
        crack: None,
    };

    let resolved_quoted = LaunchArguments::resolve_assets_dir(&session_quoted, &official_mc_path);
    assert_eq!(resolved_quoted, PathBuf::from("/custom/assets/directory"));
}

#[test]
fn test_ensure_assets_dir_exists_empty_fails() {
    let empty_path = PathBuf::from("");
    assert!(LaunchArguments::ensure_assets_dir_exists(&empty_path).is_err());

    let ws_path = PathBuf::from("   ");
    assert!(LaunchArguments::ensure_assets_dir_exists(&ws_path).is_err());
}

#[test]
fn test_mojang_version_detail_deserializes_asset_index() {
    let raw_json = r#"{
        "assetIndex": {
            "id": "1.20",
            "sha1": "0123456789abcdef0123456789abcdef01234567",
            "size": 398284,
            "totalSize": 673238472,
            "url": "https://piston-meta.mojang.com/v1/packages/0123456789abcdef0123456789abcdef01234567/1.20.json"
        },
        "downloads": {
            "client": {
                "sha1": "client_sha1",
                "size": 123456,
                "url": "https://example.com/client.jar"
            }
        }
    }"#;

    let detail: Result<VersionDetail, _> = serde_json::from_str(raw_json);
    assert!(detail.is_ok(), "Failed to parse VersionDetail: {:?}", detail.err());
    let detail = detail.unwrap();
    assert!(detail.asset_index.is_some(), "assetIndex must be parsed");
    let asset_index = detail.asset_index.unwrap();
    assert_eq!(asset_index.id, "1.20");
    assert_eq!(asset_index.sha1.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
    assert_eq!(asset_index.size, Some(398284));
    assert_eq!(asset_index.total_size, Some(673238472));
    assert_eq!(
        asset_index.url.as_deref(),
        Some("https://piston-meta.mojang.com/v1/packages/0123456789abcdef0123456789abcdef01234567/1.20.json")
    );
}

#[test]
fn test_version_manifest_deserializes_asset_index_without_url() {
    let raw_json = r#"{
        "id": "1.20.1-forge-47.2.0",
        "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
        "libraries": [],
        "assetIndex": {
            "id": "1.20"
        }
    }"#;

    let manifest: Result<VersionManifest, _> = serde_json::from_str(raw_json);
    assert!(manifest.is_ok(), "Manifest with assetIndex lacking url must parse: {:?}", manifest.err());
    let manifest = manifest.unwrap();
    assert!(manifest.asset_index.is_some());
    let ai = manifest.asset_index.unwrap();
    assert_eq!(ai.id, "1.20");
    assert_eq!(ai.url, None);
}

#[tokio::test]
async fn test_asset_manager_directories_and_index_file_placement() {
    let test_root = std::env::temp_dir().join(format!("test_assets_mgr_{}", uuid::Uuid::new_v4().simple()));
    let custom_assets = test_root.join("custom_assets");

    // Test AssetManager path resolution
    let mgr_custom = AssetManager::new_with_assets_dir(custom_assets.clone());
    assert_eq!(mgr_custom.assets_dir(), &custom_assets);

    let mc_root = test_root.join(".minecraft");
    let mgr_mc = AssetManager::new(mc_root.clone());
    assert_eq!(mgr_mc.assets_dir(), &mc_root.join("assets"));

    // Write a mock index file to simulate already-fetched or placed index
    let index_dir = custom_assets.join("indexes");
    std::fs::create_dir_all(&index_dir).unwrap();
    let index_file = index_dir.join("test_index.json");
    std::fs::write(&index_file, r#"{"objects": {}}"#).unwrap();

    let asset_ref = AssetIndexReference {
        id: "test_index".to_string(),
        sha1: None,
        size: None,
        url: None,
        total_size: None,
    };

    let res = mgr_custom.ensure_assets(None, &asset_ref).await;
    assert!(res.is_ok(), "ensure_assets should succeed with existing index file: {:?}", res.err());

    // Assert that indexes and objects directories exist
    assert!(custom_assets.join("indexes").exists());
    assert!(custom_assets.join("objects").exists());
    assert!(index_file.exists());

    let _ = std::fs::remove_dir_all(&test_root);
}

#[tokio::test]
async fn test_asset_manager_recovers_from_empty_corrupted_index_file() {
    let test_root = std::env::temp_dir().join(format!("test_assets_recover_{}", uuid::Uuid::new_v4().simple()));
    let custom_assets = test_root.join("custom_assets");
    let mgr = AssetManager::new_with_assets_dir(custom_assets.clone());

    // Pre-create a 0-byte corrupt index file
    let index_dir = custom_assets.join("indexes");
    std::fs::create_dir_all(&index_dir).unwrap();
    let index_file = index_dir.join("corrupt_index.json");
    std::fs::write(&index_file, "").unwrap(); // Empty file

    let asset_ref = AssetIndexReference {
        id: "corrupt_index".to_string(),
        sha1: None,
        size: None,
        url: None,
        total_size: None,
    };

    // ensure_assets should detect empty/missing and write a valid fallback without failing
    let res = mgr.ensure_assets(None, &asset_ref).await;
    assert!(res.is_ok(), "ensure_assets should recover from empty index file: {:?}", res.err());
    assert!(index_file.exists());
    let content = std::fs::read_to_string(&index_file).unwrap();
    assert_eq!(content, r#"{"objects":{}}"#);

    let _ = std::fs::remove_dir_all(&test_root);
}

#[test]
fn test_session_deserialization_assets_path_alias() {
    let json_camel = r#"{
        "name": "Camel Session",
        "minecraft": "1.20.1",
        "syncDir": "mods",
        "syncUrl": "https://example.com/sync",
        "welcome": "Hi",
        "jvmArg": "",
        "credits": "",
        "isDefault": false,
        "assetsPath": "/path/to/camel"
    }"#;
    let s_camel: Session = serde_json::from_str(json_camel).unwrap();
    assert_eq!(s_camel.assets_path.as_deref(), Some("/path/to/camel"));

    let json_snake = r#"{
        "name": "Snake Session",
        "minecraft": "1.20.1",
        "syncDir": "mods",
        "syncUrl": "https://example.com/sync",
        "welcome": "Hi",
        "jvmArg": "",
        "credits": "",
        "isDefault": false,
        "assets_path": "/path/to/snake"
    }"#;
    let s_snake: Session = serde_json::from_str(json_snake).unwrap();
    assert_eq!(s_snake.assets_path.as_deref(), Some("/path/to/snake"));
}

#[tokio::test]
async fn test_asset_manager_replaces_dummy_index_when_checksum_provided() {
    let test_root = std::env::temp_dir().join(format!("test_assets_dummy_{}", uuid::Uuid::new_v4().simple()));
    let custom_assets = test_root.join("custom_assets");
    let index_dir = custom_assets.join("indexes");
    std::fs::create_dir_all(&index_dir).unwrap();
    let index_file = index_dir.join("test_dummy.json");

    // Write dummy fallback
    std::fs::write(&index_file, r#"{"objects":{}}"#).unwrap();

    // Check validity against a real expected size & sha1
    let expected_sha1 = Some(app_lib::core::download::Checksum::Sha1(
        "da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string(),
    ));
    let is_valid = app_lib::core::download::DownloadEngine::is_file_valid(
        &index_file,
        Some(12345),
        &expected_sha1,
    )
    .await;

    // Dummy index of 14 bytes should NOT be considered valid when expected size is 12345
    assert!(!is_valid, "Dummy fallback index must not pass validity check when real size/checksum expected");

    let _ = std::fs::remove_dir_all(&test_root);
}

#[test]
fn test_launch_arguments_parses_asset_index_flag_variants() {
    let args_space = vec![
        "--username".to_string(),
        "Player".to_string(),
        "--assetIndex".to_string(),
        "1.20.1".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_asset_index_id(&args_space),
        Some("1.20.1".to_string())
    );

    let args_equals = vec![
        "--username".to_string(),
        "Player".to_string(),
        "--assetIndex=1.20.1".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_asset_index_id(&args_equals),
        Some("1.20.1".to_string())
    );

    let args_quoted = vec![
        "--assetIndex=\"1.20.1\"".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_asset_index_id(&args_quoted),
        Some("1.20.1".to_string())
    );

    let args_missing = vec![
        "--username".to_string(),
        "Player".to_string(),
    ];
    assert_eq!(LaunchArguments::extract_asset_index_id(&args_missing), None);

    let args_next_flag = vec![
        "--assetIndex".to_string(),
        "--gameDir".to_string(),
    ];
    assert_eq!(LaunchArguments::extract_asset_index_id(&args_next_flag), None);

    // Sequential override: last valid flag wins, matching joptsimple OptionSet.valueOf semantics
    let args_sequential = vec![
        "--assetIndex".to_string(),
        "1.20".to_string(),
        "--assetIndex".to_string(),
        "1.20.1".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_asset_index_id(&args_sequential),
        Some("1.20.1".to_string())
    );

    // Trailing flag without value or followed by another flag should NOT clobber previous valid value
    let args_trailing_invalid = vec![
        "--assetIndex".to_string(),
        "1.20.1".to_string(),
        "--assetIndex".to_string(),
        "--gameDir".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_asset_index_id(&args_trailing_invalid),
        Some("1.20.1".to_string())
    );
}

#[test]
fn test_launch_arguments_extracts_assets_dir_variants() {
    let args_space = vec![
        "--username".to_string(),
        "Player".to_string(),
        "--assetsDir".to_string(),
        "/path/to/assets".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_assets_dir(&args_space),
        Some(PathBuf::from("/path/to/assets"))
    );

    let args_equals = vec![
        "--assetsDir=/path/to/assets".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_assets_dir(&args_equals),
        Some(PathBuf::from("/path/to/assets"))
    );

    let args_quoted = vec![
        "--assetsDir=\"/path/to/assets\"".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_assets_dir(&args_quoted),
        Some(PathBuf::from("/path/to/assets"))
    );

    // Sequential override: last valid flag wins, matching joptsimple OptionSet.valueOf semantics
    let args_sequential = vec![
        "--assetsDir".to_string(),
        "/first/path".to_string(),
        "--assetsDir=/second/path".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_assets_dir(&args_sequential),
        Some(PathBuf::from("/second/path"))
    );

    // Trailing flag without value or followed by another flag should NOT clobber previous valid value
    let args_trailing_invalid = vec![
        "--assetsDir".to_string(),
        "/valid/path".to_string(),
        "--assetsDir".to_string(),
        "--gameDir".to_string(),
    ];
    assert_eq!(
        LaunchArguments::extract_assets_dir(&args_trailing_invalid),
        Some(PathBuf::from("/valid/path"))
    );
}

#[tokio::test]
async fn test_asset_manager_recovers_from_corrupt_html_index_file() {
    let test_dir = std::env::temp_dir().join(format!("test_corrupt_index_{}", uuid::Uuid::new_v4().simple()));
    let index_dir = test_dir.join("indexes");
    std::fs::create_dir_all(&index_dir).unwrap();
    let index_file = index_dir.join("corrupt.json");

    // Write corrupt non-JSON (e.g. HTML error page)
    std::fs::write(&index_file, "<!DOCTYPE html><html><body>502 Bad Gateway</body></html>").unwrap();

    let mgr = AssetManager::new_with_assets_dir(test_dir.clone());
    let asset_ref = AssetIndexReference {
        id: "corrupt".to_string(),
        sha1: None,
        size: None,
        url: None,
        total_size: None,
    };

    // AssetManager::ensure_assets must detect corrupt HTML and recover with valid JSON fallback
    let res = mgr.ensure_assets(None, &asset_ref).await;
    assert!(res.is_ok(), "ensure_assets must succeed when recovering from corrupt HTML index: {:?}", res.err());

    let recovered_content = std::fs::read_to_string(&index_file).unwrap();
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&recovered_content);
    assert!(parsed.is_ok(), "Recovered index must be valid JSON");
    assert!(parsed.unwrap().get("objects").is_some(), "Recovered index must contain objects key");

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_launch_service_assets_dir_guarantee_creates_subdirectories() {
    let test_dir = std::env::temp_dir().join(format!("test_launch_subdirs_{}", uuid::Uuid::new_v4().simple()));
    let assets_dir = test_dir.join("mc_assets");

    assert!(!assets_dir.exists());

    let minecraft_args = vec![
        "--username".to_string(),
        "Player".to_string(),
        "--assetsDir".to_string(),
        assets_dir.to_string_lossy().to_string(),
    ];

    let extracted = LaunchArguments::extract_assets_dir(&minecraft_args);
    assert_eq!(extracted, Some(assets_dir.clone()));

    let guarantee_res = LaunchArguments::guarantee_assets_dir_and_subdirs(&extracted.unwrap());
    assert!(guarantee_res.is_ok());

    assert!(assets_dir.exists(), "Root assetsDir must exist");
    assert!(assets_dir.join("indexes").exists(), "indexes directory must exist");
    assert!(assets_dir.join("objects").exists(), "objects directory must exist");

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[tokio::test]
async fn test_asset_manager_handles_empty_placeholder_with_url() {
    let test_dir = std::env::temp_dir().join(format!("test_placeholder_upgrade_{}", uuid::Uuid::new_v4().simple()));
    let index_dir = test_dir.join("indexes");
    std::fs::create_dir_all(&index_dir).unwrap();
    let index_file = index_dir.join("placeholder_test.json");

    // Write minimal empty placeholder (such as created by offline fallback)
    std::fs::write(&index_file, r#"{"objects":{}}"#).unwrap();

    let mgr = AssetManager::new_with_assets_dir(test_dir.clone());
    let asset_ref = AssetIndexReference {
        id: "placeholder_test".to_string(),
        sha1: None,
        size: None,
        // Unreachable local port to simulate network failure after clearing placeholder
        url: Some("http://127.0.0.1:1/unreachable.json".to_string()),
        total_size: None,
    };

    // ensure_assets should recognize the placeholder, attempt download, handle network failure cleanly,
    // and guarantee a valid fallback exists on disk
    let res = mgr.ensure_assets(None, &asset_ref).await;
    assert!(res.is_ok(), "ensure_assets must succeed gracefully: {:?}", res.err());
    assert!(index_file.exists(), "Index file must exist on disk");

    let content = std::fs::read_to_string(&index_file).unwrap();
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&content);
    assert!(parsed.is_ok(), "Index must be valid JSON");
    assert!(parsed.unwrap().get("objects").is_some());

    let _ = std::fs::remove_dir_all(&test_dir);
}

#[test]
fn test_guarantee_assets_dir_and_subdirs_empty_path_errors() {
    let empty = PathBuf::from("");
    assert!(LaunchArguments::guarantee_assets_dir_and_subdirs(&empty).is_err());

    let whitespace = PathBuf::from("   ");
    assert!(LaunchArguments::guarantee_assets_dir_and_subdirs(&whitespace).is_err());

    let quotes_only = PathBuf::from("\"\"");
    assert!(LaunchArguments::guarantee_assets_dir_and_subdirs(&quotes_only).is_err());
}




