/**
 * Handles building JVM arguments and launching the game.
 */
pub mod args;
pub mod models;

use crate::core::launch::args::LaunchArguments;
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct LaunchService;

impl LaunchService {
    /**
     * Launches the game process.
     */
    pub fn launch(&self, args: LaunchArguments, show_logs: bool, app_handle: &tauri::AppHandle) -> Result<(), String> {
        // Guarantee that the directory path passed to --assetsDir exists on disk prior to process execution
        if let Some(assets_dir) = LaunchArguments::extract_assets_dir(&args.minecraft_args) {
            LaunchArguments::guarantee_assets_dir_and_subdirs(&assets_dir)
                .map_err(|e| format!("Cannot launch: failed to guarantee assets directory {:?}: {}", assets_dir.display(), e))?;
        }

        let jvm_args = args.build();
        
        let log_file_path = args.game_dir.join("launcher_debug.log");
        log::info!("Game logs will be written to: {:?}", log_file_path);

        let mut cmd = if !args.wrapper_command.is_empty() {
            let mut parts = args.wrapper_command.split_whitespace();
            let wrapper_cmd = parts.next().unwrap();
            let mut c = Command::new(wrapper_cmd);
            c.args(parts);
            c.arg(&args.java_path);
            c
        } else {
            Command::new(&args.java_path)
        };

        cmd.args(jvm_args);
        cmd.current_dir(&args.game_dir);

        // --- Environment Isolation ---
        #[cfg(unix)]
        {
            cmd.env_remove("LD_LIBRARY_PATH");
            let mut lib_paths: Vec<String> = Vec::new();

            if let Some(ref java_home) = args.java_home {
                let java_lib = java_home.join("lib");
                if java_lib.exists() {
                    lib_paths.push(java_lib.to_string_lossy().into_owned());
                }
                let java_lib_server = java_home.join("lib/server");
                if java_lib_server.exists() {
                    lib_paths.push(java_lib_server.to_string_lossy().into_owned());
                }
            }

            if !lib_paths.is_empty() {
                let ld_path = lib_paths.join(":");
                log::info!("Setting isolated LD_LIBRARY_PATH: {}", ld_path);
                cmd.env("LD_LIBRARY_PATH", ld_path);
            }
        }

        // --- Output Redirection ---
        // We use pipes to stream logs to the UI AND write to file
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        match cmd.spawn() {
            Ok(mut child) => {
                log::info!("Game process spawned successfully with PID: {:?}", child.id());
                
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                let handle_out = app_handle.clone();
                let handle_err = app_handle.clone();
                let log_file_path_out = log_file_path.clone();
                let log_file_path_err = log_file_path.clone();

                // Thread for STDOUT
                std::thread::spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    use std::fs::OpenOptions;
                    use tauri::Emitter;
                    let mut file: Option<std::fs::File> = OpenOptions::new().append(true).create(true).open(&log_file_path_out).ok();
                    let reader = BufReader::new(stdout);
                    for l in reader.lines().map_while(Result::ok) {
                        if show_logs { let _ = handle_out.emit("game-log", &l); }
                        if let Some(f) = file.as_mut() {
                            let _ = writeln!(f, "{}", l);
                            let _ = f.flush();
                        }
                    }
                });

                // Thread for STDERR
                std::thread::spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    use std::fs::OpenOptions;
                    use tauri::Emitter;
                    let mut file: Option<std::fs::File> = OpenOptions::new().append(true).create(true).open(&log_file_path_err).ok();
                    let reader = BufReader::new(stderr);
                    for l in reader.lines().map_while(Result::ok) {
                        if show_logs { let _ = handle_err.emit("game-log", format!("[ERROR] {}", l)); }
                        if let Some(f) = file.as_mut() {
                            let _ = writeln!(f, "[ERROR] {}", l);
                            let _ = f.flush();
                        }
                    }
                });

                let app_handle_wait = app_handle.clone();
                // Thread to wait for process exit
                std::thread::spawn(move || {
                    use tauri::Emitter;
                    match child.wait() {
                        Ok(status) => {
                            log::info!("Game process exited with status: {}", status);
                            let _ = app_handle_wait.emit("game-log", format!("--- Process exited with status {} ---", status));
                        },
                        Err(e) => log::error!("Error waiting for game process: {}", e),
                    }
                });

                Ok(())
            }
            Err(e) => {
                let err_msg = format!("Failed to spawn game process: {}. Java path: {:?}", e, args.java_path);
                log::error!("{}", err_msg);
                Err(err_msg)
            }
        }
    }
}
