use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};
use oc_runner::{get_endpoint_help, list_endpoints, list_plugins, run_oce_file, Db, PluginInstallService};

#[derive(Parser)]
#[command(name = "oc", about = "Open Choice CLI — run .oce files and explore plugins", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to the Open Choice database (defaults to the standard install location)
    #[arg(long, global = true)]
    db: Option<PathBuf>,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    /// Human-readable text (default)
    Human,
    /// Raw NDJSON events — one JSON object per line (oc run only)
    Ndjson,
    /// JSON array or object on stdout (discovery and install commands)
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Run a .oce file (all tasks, or a single named task)
    Run {
        /// Path to the .oce file
        path: PathBuf,

        /// Run only this task (omit to run all tasks in pipeline order)
        #[arg(long)]
        task: Option<String>,

        /// Output format: 'human' (default) or 'ndjson' for machine-readable events
        #[arg(long, value_enum, default_value = "human")]
        output: OutputFormat,
    },

    /// List all installed plugins
    Plugins {
        /// Output format: 'human' (default) or 'json'
        #[arg(long, value_enum, default_value = "human")]
        output: OutputFormat,
    },

    /// List endpoints for a plugin
    Endpoints {
        /// Plugin ID (e.g. com.example.spss-reader)
        plugin_id: String,

        /// Output format: 'human' (default) or 'json'
        #[arg(long, value_enum, default_value = "human")]
        output: OutputFormat,
    },

    /// Show help for a plugin or a specific endpoint
    Help {
        /// Plugin ID
        plugin_id: String,

        /// Endpoint ID (omit to list all endpoints with summaries)
        #[arg(long)]
        endpoint: Option<String>,

        /// Output format: 'human' (default) or 'json'
        #[arg(long, value_enum, default_value = "human")]
        output: OutputFormat,
    },

    /// Install a .ocplugin package file
    Install {
        /// Path to the .ocplugin file
        path: PathBuf,

        /// Allow installing unsigned or unrecognized-publisher packages
        #[arg(long)]
        developer_mode: bool,

        /// Acknowledge arbitrary-code-execution risk profile
        #[arg(long)]
        risk_acknowledged: bool,

        /// Acknowledge that this update changes the plugin's declared capabilities
        #[arg(long)]
        capabilities_change_acknowledged: bool,

        /// Override the plugins directory (defaults to the standard install location)
        #[arg(long)]
        plugins_dir: Option<PathBuf>,

        /// Output format: 'human' (default) or 'json'
        #[arg(long, value_enum, default_value = "human")]
        output: OutputFormat,
    },

    /// Uninstall a plugin
    Uninstall {
        /// Plugin ID to uninstall
        plugin_id: String,

        /// Also delete the plugin binary and install directory from disk
        #[arg(long)]
        remove_files: bool,

        /// Override the plugins directory
        #[arg(long)]
        plugins_dir: Option<PathBuf>,
    },

    /// Verify the binary integrity of an installed plugin
    Verify {
        /// Plugin ID to verify
        plugin_id: String,

        /// Override the plugins directory
        #[arg(long)]
        plugins_dir: Option<PathBuf>,
    },
}

fn open_db(db_path: Option<PathBuf>) -> Db {
    let path = db_path
        .or_else(Db::default_path)
        .unwrap_or_else(|| {
            eprintln!("error: could not determine the Open Choice database path.");
            eprintln!("hint:  pass --db <path> explicitly.");
            process::exit(1);
        });
    Db::open(&path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    })
}

fn open_plugins_dir(plugins_dir: Option<PathBuf>) -> PathBuf {
    plugins_dir
        .or_else(Db::default_plugins_dir)
        .unwrap_or_else(|| {
            eprintln!("error: could not determine the plugins directory.");
            eprintln!("hint:  pass --plugins-dir <path> explicitly.");
            process::exit(1);
        })
}

fn main() {
    let cli = Cli::parse();
    let db_override = cli.db;

    match cli.command {
        Command::Run { path, task, output } => {
            let db = open_db(db_override);

            if !path.exists() {
                eprintln!("error: file not found: {}", path.display());
                process::exit(1);
            }

            // Track whether any task-level failure event was emitted.
            let task_failed = Arc::new(AtomicBool::new(false));
            let task_failed_cb = task_failed.clone();

            let on_event: Arc<dyn Fn(String) + Send + Sync> = match output {
                OutputFormat::Ndjson | OutputFormat::Json => Arc::new(move |json: String| {
                    println!("{}", json);
                    // Still track failures so we can exit non-zero.
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) {
                        if v["kind"].as_str() == Some("event.run.failed") {
                            task_failed_cb.store(true, Ordering::Relaxed);
                        }
                    }
                }),
                OutputFormat::Human => Arc::new(move |json: String| {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                        if parsed["kind"].as_str() == Some("event.run.failed") {
                            task_failed_cb.store(true, Ordering::Relaxed);
                        }
                        print_event(&parsed);
                    }
                }),
            };

            if let Err(e) = run_oce_file(&path, task.as_deref(), &db, Some(on_event)) {
                eprintln!("error: {}", e);
                process::exit(1);
            }

            if task_failed.load(Ordering::Relaxed) {
                process::exit(1);
            }
        }

        Command::Plugins { output } => {
            let db = open_db(db_override);
            match list_plugins(&db) {
                Ok(plugins) => match output {
                    OutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&plugins).unwrap());
                    }
                    _ if plugins.is_empty() => println!("No plugins installed."),
                    _ => {
                        let col_w = plugins.iter().map(|p| p.plugin_id.len()).max().unwrap_or(10).max(10);
                        println!("{:<width$}  {:12}  {}", "PLUGIN", "STATUS", "VERSION", width = col_w);
                        println!("{}", "-".repeat(col_w + 30));
                        for p in &plugins {
                            let status = if !p.enabled { "disabled".to_string() } else { p.trust_status.clone() };
                            println!("{:<width$}  {:12}  {}", p.plugin_id, status, p.version, width = col_w);
                        }
                    }
                },
                Err(e) => { eprintln!("error: {}", e); process::exit(1); }
            }
        }

        Command::Endpoints { plugin_id, output } => {
            let db = open_db(db_override);
            match list_endpoints(&db, &plugin_id) {
                Ok(endpoints) => match output {
                    OutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&endpoints).unwrap());
                    }
                    _ if endpoints.is_empty() => println!("No endpoints found for '{}'.", plugin_id),
                    _ => {
                        println!("Endpoints for {}:\n", plugin_id);
                        let col_w = endpoints.iter().map(|e| e.endpoint_id.len()).max().unwrap_or(10).max(10);
                        for ep in &endpoints {
                            let summary = ep.summary.as_deref().unwrap_or("(no summary)");
                            println!("  {:<width$}  {}", ep.endpoint_id, summary, width = col_w);
                        }
                        println!("\nRun `oc help {} --endpoint <endpoint>` for details.", plugin_id);
                    }
                },
                Err(e) => { eprintln!("error: {}", e); process::exit(1); }
            }
        }

        Command::Help { plugin_id, endpoint, output } => {
            let db = open_db(db_override);
            if let Some(ep_id) = endpoint {
                match get_endpoint_help(&db, &plugin_id, &ep_id) {
                    Ok(help) => match output {
                        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&help).unwrap()),
                        _ => print_endpoint_help(&plugin_id, &help),
                    },
                    Err(e) => { eprintln!("error: {}", e); process::exit(1); }
                }
            } else {
                match list_endpoints(&db, &plugin_id) {
                    Ok(endpoints) => match output {
                        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&endpoints).unwrap()),
                        _ if endpoints.is_empty() => println!("No endpoints found for '{}'.", plugin_id),
                        _ => {
                            println!("Plugin: {}\n", plugin_id);
                            println!("Endpoints:");
                            let col_w = endpoints.iter().map(|e| e.endpoint_id.len()).max().unwrap_or(10).max(10);
                            for ep in &endpoints {
                                let summary = ep.summary.as_deref().unwrap_or("(no summary)");
                                println!("  {:<width$}  {}", ep.endpoint_id, summary, width = col_w);
                            }
                            println!("\nRun `oc help {} --endpoint <endpoint>` for full parameter docs.", plugin_id);
                        }
                    },
                    Err(e) => { eprintln!("error: {}", e); process::exit(1); }
                }
            }
        }

        Command::Install { path, developer_mode, risk_acknowledged, capabilities_change_acknowledged, plugins_dir, output } => {
            let db = open_db(db_override);

            if !path.exists() {
                eprintln!("error: file not found: {}", path.display());
                process::exit(1);
            }

            let svc = PluginInstallService::new(
                db,
                open_plugins_dir(plugins_dir),
                developer_mode,
                risk_acknowledged,
                capabilities_change_acknowledged,
            );

            match svc.install_package(&path) {
                Ok(result) => match output {
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
                    _ => {
                        println!("Plugin installed successfully.");
                        println!("  Plugin ID:        {}", result.plugin_id);
                        println!("  Installation ID:  {}", result.installation_id);
                        println!("  Trust status:     {}", result.trust_status);
                        println!("  Trust tier:       {}", result.trust_tier);
                        println!("  Signature:        {}", result.signature_status);
                        for warn in &result.warnings {
                            eprintln!("warning: {}", warn);
                        }
                    }
                },
                Err(e) => {
                    eprintln!("error: {}", e.message);
                    process::exit(1);
                }
            }
        }

        Command::Uninstall { plugin_id, remove_files, plugins_dir } => {
            let db = open_db(db_override);
            let svc = PluginInstallService::new(
                db,
                open_plugins_dir(plugins_dir),
                false, false, false,
            );

            match svc.uninstall_plugin(&plugin_id, remove_files) {
                Ok(()) => {
                    println!("Plugin '{}' uninstalled.", plugin_id);
                    if remove_files {
                        println!("Install directory removed from disk.");
                    } else {
                        println!("hint: pass --remove-files to also delete the plugin binary.");
                    }
                }
                Err(e) => { eprintln!("error: {}", e.message); process::exit(1); }
            }
        }

        Command::Verify { plugin_id, plugins_dir } => {
            let db = open_db(db_override);
            let svc = PluginInstallService::new(
                db,
                open_plugins_dir(plugins_dir),
                false, false, false,
            );

            match svc.verify_installed(&plugin_id) {
                Ok(true) => {
                    println!("Plugin '{}': binary hash OK.", plugin_id);
                }
                Ok(false) => {
                    eprintln!("error: plugin '{}' binary hash FAILED — installation quarantined.", plugin_id);
                    process::exit(1);
                }
                Err(e) => { eprintln!("error: {}", e.message); process::exit(1); }
            }
        }
    }
}

// ── Event rendering (mirrors Open Choice console handleEventLine) ──────────────

fn print_event(obj: &serde_json::Value) {
    let kind = obj["kind"].as_str().unwrap_or("");
    let payload = &obj["payload"];
    let label = obj["label"].as_str();

    let prefix = label.map(|l| format!("[{}] ", l)).unwrap_or_default();

    match kind {
        "event.run.started" => {
            let name = payload["label"].as_str()
                .or_else(|| payload["argv"].get(0).and_then(|v| v.as_str()))
                .unwrap_or("task");
            println!("{}[start] {}", prefix, name);
        }
        "event.run.progress" => {
            if let Some(line) = payload["stdout_line"].as_str() {
                if !line.is_empty() {
                    println!("{}{}", prefix, line);
                }
            }
        }
        "event.run.finished" => {
            let summary = payload["summary"].as_str().unwrap_or("Finished");
            println!("{}[done] {}", prefix, summary);
        }
        "event.run.failed" => {
            let msg = payload["error"].as_str().unwrap_or("Run failed");
            eprintln!("{}[error] {}", prefix, msg);
        }
        "event.run.cancelled" => {
            let reason = payload["reason"].as_str().unwrap_or("Cancelled");
            println!("{}[cancelled] {}", prefix, reason);
        }
        "event.artifact.created" => {
            if let Some(path) = payload["artifact"]["path"]["path"].as_str() {
                println!("{}[artifact] {}", prefix, path);
            }
        }
        "event.message.warning" | "event.message.error" | "event.log.line" => {
            if let Some(msg) = payload["message"].as_str() {
                let severity = payload["severity"].as_str().unwrap_or("info");
                if matches!(severity, "warning" | "error" | "fatal")
                    || matches!(kind, "event.message.warning" | "event.message.error")
                {
                    eprintln!("{}{}", prefix, msg);
                } else {
                    println!("{}{}", prefix, msg);
                }
            }
        }
        _ => {
            let text = payload["summary"].as_str()
                .or_else(|| payload["error"].as_str())
                .or_else(|| payload["message"].as_str());
            if let Some(t) = text {
                if !t.is_empty() {
                    println!("{}{}", prefix, t);
                }
            }
        }
    }
}

// ── Help renderer ─────────────────────────────────────────────────────────────

fn print_endpoint_help(plugin_id: &str, help: &oc_runner::EndpointHelp) {
    println!("{} › {}", plugin_id, help.endpoint_id);
    println!();
    println!("{}", help.summary);

    if let Some(ref usage) = help.usage {
        println!();
        println!("Usage:");
        println!("  {}", usage);
    }

    if !help.parameters.is_empty() {
        println!();
        println!("Parameters:");
        let col_w = help.parameters.iter().map(|p| p.name.len()).max().unwrap_or(8).max(8);
        for param in &help.parameters {
            let req = if param.required { " (required)" } else { "" };
            println!("  {:<width$}  {}{}", param.name, param.description, req, width = col_w);
            if !param.accepted_values.is_empty() {
                println!("  {:<width$}  Values: {}", "", param.accepted_values.join(", "), width = col_w);
            }
        }
    }

    if !help.output_notes.is_empty() {
        println!();
        println!("Output:");
        for note in &help.output_notes {
            println!("  • {}", note);
        }
    }

    if !help.notes.is_empty() {
        println!();
        println!("Notes:");
        for note in &help.notes {
            println!("  • {}", note);
        }
    }
}
