use std::process::Command;
use clap::Parser;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write, Read};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader as TokioBufReader};
use tokio::process::Command as TokioCommand;
use warp::Filter;
use std::path::PathBuf;
use std::fs;

#[derive(Debug, Clone)]
struct RestartRequest {
    process_id: String,
    delay_seconds: u64,
    reason: String,
}

#[derive(Parser)]
#[command(name = "exeio")]
#[command(about = "A process supervisor written in rust to help server programmers to run processes and monitor them from outside the server through a rest API", long_about = None)]
#[command(author ="philosan")]
struct Cli {
    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind to
    #[arg(short = 'P', long="port", default_value_t = 8080)]
    port: u16,

    /// API key for authentication (if not provided, a random key will be generated)
    #[arg(short = 'k', long="api-key")]
    api_key: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessConfig {
    id: String,
    command: String,
    args: Vec<String>,
    working_dir: Option<String>,
    auto_restart: bool,
    log_file: String,
    periodic: bool,
    period_seconds: Option<u64>,
}

#[derive(Debug)]
struct ManagedProcess {
    config: ProcessConfig,
    child: Option<Child>,
    log_file: File,
    stdin_sender: Option<std::sync::mpsc::Sender<String>>,
    run_count: u64,
    last_run: Option<chrono::DateTime<chrono::Utc>>,
    periodic_handle: Option<tokio::task::JoinHandle<()>>,
    status: ProcessStatus,
    auto_restart_handle: Option<tokio::task::JoinHandle<()>>, // Handle for auto-restart monitor
    last_exit_time: Option<chrono::DateTime<chrono::Utc>>, // Track when process last exited
}

#[derive(Debug, Clone, Serialize)]
enum ProcessStatus {
    Running,
    Stopped,
    WaitingForPeriod,
    Failed,
    ManuallyStopped,
}

type ProcessMap = Arc<Mutex<HashMap<String, ManagedProcess>>>;

#[derive(Deserialize)]
struct AddProcessRequest {
    id: String,
    command: String,
    args: Vec<String>,
    working_dir: Option<String>,
    auto_restart: bool,
    save_for_next_run: bool,
    periodic: Option<bool>,
    period_seconds: Option<u64>,
}

#[derive(Deserialize)]
struct ProcessInputRequest {
    input: String,
}

#[derive(Deserialize)]
struct PaginationParams {
    page: Option<usize>,
    page_size: Option<usize>,
}

#[derive(Serialize)]
struct ApiResponse {
    success: bool,
    message: String,
}

// Authentication-related structures and functions
#[derive(Serialize)]
struct AuthError {
    success: bool,
    message: String,
}

fn generate_api_key() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    
    format!("exeio_philo{:x}", hasher.finish())
}

fn with_auth(api_key: Arc<String>) -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
    warp::header::optional::<String>("exeio-api-key")
        .and(warp::any().map(move || api_key.clone()))
        .and_then(validate_api_key)
        .untuple_one()
}

async fn validate_api_key(provided_key: Option<String>, expected_key: Arc<String>) -> Result<(), warp::Rejection> {
    match provided_key {
        Some(key) if key == *expected_key => Ok(()),
        _ => Err(warp::reject::custom(AuthenticationError))
    }
}

#[derive(Debug)]
struct AuthenticationError;

impl warp::reject::Reject for AuthenticationError {}

async fn handle_auth_error(err: warp::Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    if err.find::<AuthenticationError>().is_some() {
        let response = AuthError {
            success: false,
            message: "Invalid or missing API key. Provide a valid key in the 'exeio-api-key' header.".to_string(),
        };
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::UNAUTHORIZED,
        ))
    } else {
        let response = AuthError {
            success: false,
            message: "Internal server error".to_string(),
        };
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))
    }
}

// Thread-safe logging and config management
struct SafeLogger {
    log_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl SafeLogger {
    fn new() -> Self {
        Self {
            log_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_log_lock(&self, log_path: &str) -> Arc<Mutex<()>> {
        let mut locks = self.log_locks.lock().unwrap();
        locks.entry(log_path.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn safe_append_log(&self, log_path: &str, content: &str) -> Result<(), std::io::Error> {
        let lock = self.get_log_lock(log_path);
        let _guard = lock.lock().unwrap();
        
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        file.write_all(content.as_bytes())?;
        file.flush()?;
        Ok(())
    }
}

struct SafeConfigManager {
    config_lock: Arc<RwLock<()>>,
    config_path: PathBuf,
}

impl SafeConfigManager {
    fn new() -> Self {
        Self {
            config_lock: Arc::new(RwLock::new(())),
            config_path: get_config_path(),
        }
    }

    fn load_configs(&self) -> Vec<ProcessConfig> {
        let _read_guard = self.config_lock.read().unwrap();
        
        match std::fs::read_to_string(&self.config_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save_configs(&self, configs: &[ProcessConfig]) -> Result<(), Box<dyn std::error::Error>> {
        let _write_guard = self.config_lock.write().unwrap();
        
        // Use atomic write: write to temp file, then rename
        let temp_path = self.config_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(configs)?;
        
        std::fs::write(&temp_path, json)?;
        std::fs::rename(&temp_path, &self.config_path)?;
        
        Ok(())
    }

    fn save_process_config(&self, config: &ProcessConfig) -> Result<(), Box<dyn std::error::Error>> {
        let mut configs = self.load_configs();
        configs.retain(|c| c.id != config.id);
        configs.push(config.clone());
        self.save_configs(&configs)
    }

    fn remove_process_config(&self, id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut configs = self.load_configs();
        configs.retain(|c| c.id != id);
        self.save_configs(&configs)
    }
}

// Global instances
lazy_static::lazy_static! {
    static ref SAFE_LOGGER: SafeLogger = SafeLogger::new();
    static ref CONFIG_MANAGER: SafeConfigManager = SafeConfigManager::new();
    static ref RESTART_SENDER: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<RestartRequest>>>> = 
        Arc::new(Mutex::new(None));
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Check for single instance before doing anything else
    if let Err(e) = ensure_single_instance() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Generate or use provided API key
    let api_key = Arc::new(
        cli.api_key.unwrap_or_else(|| generate_api_key())
    );

    let processes: ProcessMap = Arc::new(Mutex::new(HashMap::new()));
    let host = Arc::new(cli.host.clone());
    
    let _exeio_log_path = init_exeio_log(&host, cli.port);

    // Create a clone of host for the closure
    let host_for_log = Arc::clone(&host);
    let log_filter = warp::log::custom(move |info| {
        let log_entry = format!(
            "{} {} -> {}\n",
            info.method(),
            info.path(),
            info.status()
        );
        
        println!(
            "exeio: [{}] {} {} -> {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            info.method(),
            info.path(),
            info.status()
        );
        
        log_exeio_event(&log_entry, &host_for_log, cli.port); 
    });
    
    
    // Set up restart handler
    let (restart_tx, mut restart_rx) = tokio::sync::mpsc::unbounded_channel::<RestartRequest>();
    {
        let mut sender = RESTART_SENDER.lock().unwrap();
        *sender = Some(restart_tx);
    }
    
    // Start restart handler task
    let restart_processes = processes.clone();
    let restart_host = host.clone();
    let restart_port = cli.port;
    tokio::spawn(async move {
        while let Some(request) = restart_rx.recv().await {
            tokio::time::sleep(Duration::from_secs(request.delay_seconds)).await;
            
            let restart_log = format!("[{}] SYSTEM {}:{}: {}\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), restart_host, restart_port, request.reason);
            
            // Get the config for this process
            let config = {
                let processes_lock = restart_processes.lock().unwrap();
                processes_lock.get(&request.process_id).map(|p| p.config.clone())
            };
            
            if let Some(config) = config {
                let _ = SAFE_LOGGER.safe_append_log(&config.log_file, &restart_log);
                start_process(restart_processes.clone(), config, restart_host.clone(), restart_port).await;
            }
        }
    });
    
    load_and_start_processes(processes.clone(), host.clone(), cli.port).await;
    
    // Ensure config directory exists
    let config_path = get_config_path();
    println!("Using config file: {}", config_path.display());
    
    // Setup API routes
    let processes_filter = warp::any().map(move || processes.clone());
    let host_filter = warp::any().map({
        let host = host.clone();
        move || host.clone()
    });
    let port_filter = warp::any().map(move || cli.port);
    let auth_filter = with_auth(api_key.clone());
    
    let add_process = warp::path("add")
        .and(warp::post())
        .and(auth_filter.clone())
        .and(warp::body::json())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_add_process);
    
    let restart_process = warp::path("restart")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_restart_process);
    
    let stop_process = warp::path("stop")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_stop_process);
    
    let remove_process = warp::path("remove")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_remove_process);
    
    let restart_all = warp::path("restart-all")
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_restart_all);
    
    let stop_all = warp::path("stop-all")
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and_then(handle_stop_all);
    
    let send_input = warp::path("input")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(auth_filter.clone())
        .and(warp::body::json())
        .and(processes_filter.clone())
        .and_then(handle_send_input);
    
    let clear_log = warp::path("clear-log")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and_then(handle_clear_log);
    
    let list_processes = warp::path("list")
        .and(warp::get())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and_then(handle_list_processes);

    // Use another clone of host for the info route - this one stays unprotected for health checks
    let host_for_info = Arc::clone(&host);
    let exeio_info = warp::path("info")
        .and(warp::get())
        .and(warp::any().map(move || Arc::clone(&host_for_info)))
        .and(warp::any().map(move || cli.port))
        .and_then(handle_exeio_info);
    
    let logs_route = warp::path("logs")
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(auth_filter.clone())
        .and(warp::query::<PaginationParams>())
        .and(processes_filter.clone())
        .and_then(handle_process_logs);
    
    let shutdown_route = warp::path("shutdown")
        .and(warp::post())
        .and(auth_filter.clone())
        .and(processes_filter.clone())
        .and(host_filter.clone())
        .and(port_filter.clone())
        .and_then(handle_shutdown);
    
    let routes = add_process
        .or(restart_process)
        .or(stop_process)
        .or(remove_process)
        .or(restart_all)
        .or(stop_all)
        .or(send_input)
        .or(clear_log)
        .or(list_processes)
        .or(exeio_info)
        .or(logs_route)
        .or(shutdown_route)
        .recover(handle_auth_error)
        .with(log_filter)
        .with(warp::cors().allow_any_origin());
   
    println!("Process Supervisor starting on port {} at {}", cli.port, cli.host);
    println!("API Key: {}", api_key);
    println!("NOTE: All endpoints except /info require the 'exeio-api-key' header with the above key");
    println!("Logs directory: {}", get_logs_dir().display());
    println!("Config file: {}", get_config_path().display());
    println!("  Available endpoints:");
    println!("  POST /add - Add new process (protected)");
    println!("  POST /restart/:id - Restart process (protected)");
    println!("  POST /stop/:id - Stop process (protected)");
    println!("  POST /remove/:id - Remove process (protected)");
    println!("  POST /restart-all - Restart all processes (protected)");
    println!("  POST /stop-all - Stop all processes (protected)");
    println!("  POST /input/:id - Send input to process (protected)");
    println!("  POST /clear-log/:id - Clear process log (protected)");
    println!("  GET /list - List all processes (protected)");
    println!("  GET /info - Get supervisor information (public)");
    println!("  GET /logs/:id?page=1&page_size=50 - Get paginated process logs (protected)");
    println!("  POST /shutdown - Shutdown supervisor (protected)");
    
    let addr: std::net::IpAddr = cli.host.parse()
    .unwrap_or_else(|_| {
        eprintln!("Invalid host address: {}", cli.host);
        std::process::exit(1);
    });
    warp::serve(routes)
        .run((addr, cli.port))
        .await;
}

async fn load_and_start_processes(processes: ProcessMap, host: Arc<String>, port: u16) {
    // Load configurations using the safe config manager
    let configs = CONFIG_MANAGER.load_configs();
    for config in configs {
        start_process(processes.clone(), config, host.clone(), port).await;
    }
}

async fn start_process(processes: ProcessMap, config: ProcessConfig, host: Arc<String>, port: u16) {
    let log_file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.log_file) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to open log file {}: {}", config.log_file, e);
            return;
        }
    };
    
    // Get current run count
    let current_run_count = {
        let processes_lock = processes.lock().unwrap();
        if let Some(managed_process) = processes_lock.get(&config.id) {
            managed_process.run_count
        } else {
            1
        }
    };
    
    // Log process start
    let start_log = format!("[{}] SYSTEM {}:{}: Starting process '{}' (Run #{})\n", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port, config.id, current_run_count);
    let _ = SAFE_LOGGER.safe_append_log(&config.log_file, &start_log);
    
    if config.periodic && config.period_seconds.is_some() {
        start_periodic_process(processes, config, log_file, host, port).await;
    } else {
        start_regular_process(processes, config, log_file, host, port, current_run_count).await;
    }
}

async fn start_regular_process(processes: ProcessMap, config: ProcessConfig, log_file: File, host: Arc<String>, port: u16, run_count: u64) {
    let mut cmd = Command::new(&config.command);
    cmd.args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    
    if let Some(ref dir) = config.working_dir {
        cmd.current_dir(dir);
    }
    
    match cmd.spawn() {
        Ok(mut child) => {
            let child_id = child.id();
            
            // create a channel for sending input to the child process
            let (stdin_sender, stdin_receiver) = std::sync::mpsc::channel::<String>();
            
            // Handle stdin , takes the Option
            if let Some(stdin) = child.stdin.take() {
                let mut stdin = stdin;
                thread::spawn(move || {
                    // Keep this thread alive indefinitely to avoid closing stdin
                    loop {
                        match stdin_receiver.recv() {
                            Ok(input) => {
                                if let Err(e) = writeln!(stdin, "{}", input) {
                                    eprintln!("Failed to write to process stdin: {}", e);
                                    break;
                                }
                            }
                            Err(_) => {
                                thread::sleep(Duration::from_millis(100));
                            }
                        }
                    }
                });
            }
            
            // Handle stdout
            if let Some(stdout) = child.stdout.take() {
                let log_file_path = config.log_file.clone();
                let process_id = config.id.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                let log_entry = format!("[{}] STDOUT: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), line);
                                let _ = SAFE_LOGGER.safe_append_log(&log_file_path, &log_entry);
                                println!("[{}] {}", process_id, line);
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
            
            // Handle stderr
            if let Some(stderr) = child.stderr.take() {
                let log_file_path = config.log_file.clone();
                let process_id = config.id.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => {
                                let log_entry = format!("[{}] STDERR: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), line);
                                let _ = SAFE_LOGGER.safe_append_log(&log_file_path, &log_entry);
                                eprintln!("[{}] ERROR: {}", process_id, line);
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
            
            let managed_process = ManagedProcess {
                config: config.clone(),
                child: Some(child),
                log_file,
                stdin_sender: Some(stdin_sender),
                run_count: run_count,
                last_run: Some(chrono::Utc::now()),
                periodic_handle: None,
                status: ProcessStatus::Running,
                auto_restart_handle: None,
                last_exit_time: None,
            };
            
            {
                let mut processes_lock = processes.lock().unwrap();
                processes_lock.insert(config.id.clone(), managed_process);
            }
            
            // Start auto-restart monitoring if enabled
            if config.auto_restart {
                let auto_restart_handle = start_auto_restart_monitor(
                    processes.clone(), 
                    config.clone(), 
                    host.clone(), 
                    port, 
                    child_id
                );
                
                // Store the auto-restart handle
                {
                    let mut processes_lock = processes.lock().unwrap();
                    if let Some(managed_process) = processes_lock.get_mut(&config.id) {
                        managed_process.auto_restart_handle = Some(auto_restart_handle);
                    }
                }
            }
            
            println!("Started process: {} ({})", config.id, config.command);
        }
        Err(e) => {
            eprintln!("Failed to start process {}: {}", config.id, e);
            
            let managed_process = ManagedProcess {
                config: config.clone(),
                child: None,
                log_file,
                stdin_sender: None,
                run_count: 0,
                last_run: None,
                periodic_handle: None,
                status: ProcessStatus::Failed,
                auto_restart_handle: None,
                last_exit_time: None,
            };
            
            {
                let mut processes_lock = processes.lock().unwrap();
                processes_lock.insert(config.id.clone(), managed_process);
            }
            
            // If auto-restart is enabled and process failed to start, try to restart after a delay
            if config.auto_restart {
                let config_id = config.id.clone();
                
                tokio::spawn(async move {
                    if let Some(sender) = RESTART_SENDER.lock().unwrap().as_ref() {
                        let request = RestartRequest {
                            process_id: config_id.clone(),
                            delay_seconds: 5,
                            reason: format!("Auto-restarting process '{}' after failed start", config_id),
                        };
                        let _ = sender.send(request);
                    }
                });
            }
        }
    }
}

async fn start_periodic_process(processes: ProcessMap, config: ProcessConfig, log_file: File, host: Arc<String>, port: u16) {
    let period_seconds = config.period_seconds.unwrap_or(60);
    let processes_clone = processes.clone();
    let config_clone = config.clone();
    let host_clone = host.clone();
    
    let periodic_handle = tokio::spawn(async move {
        let mut run_count = 0u64;
        
        loop {
            run_count += 1;
            
            // Log periodic run start
            let run_log = format!("[{}] SYSTEM {}:{}: Starting periodic run #{} (every {}s)\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, run_count, period_seconds);
            let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &run_log);
            
            // Update run count and status
            {
                let mut processes_lock = processes_clone.lock().unwrap();
                if let Some(managed_process) = processes_lock.get_mut(&config_clone.id) {
                    managed_process.run_count = run_count;
                    managed_process.last_run = Some(chrono::Utc::now());
                    managed_process.status = ProcessStatus::Running;
                }
            }
            
            // Run the command
            let mut cmd = TokioCommand::new(&config_clone.command);
            cmd.args(&config_clone.args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            
            if let Some(ref dir) = config_clone.working_dir {
                cmd.current_dir(dir);
            }
            
            match cmd.spawn() {
                Ok(mut child) => {
                    // Handle stdout
                    if let Some(stdout) = child.stdout.take() {
                        let log_file_path = config_clone.log_file.clone();
                        let process_id = config_clone.id.clone();
                        let run_num = run_count;
                        
                        tokio::spawn(async move {
                            let reader = TokioBufReader::new(stdout);
                            let mut lines = reader.lines();
                            
                            while let Ok(Some(line)) = lines.next_line().await {
                                let log_entry = format!("[{}] RUN#{} STDOUT: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_num, line);
                                let _ = SAFE_LOGGER.safe_append_log(&log_file_path, &log_entry);
                                println!("[{}] Run#{}: {}", process_id, run_num, line);
                            }
                        });
                    }
                    
                    // Handle stderr
                    if let Some(stderr) = child.stderr.take() {
                        let log_file_path = config_clone.log_file.clone();
                        let process_id = config_clone.id.clone();
                        let run_num = run_count;
                        
                        tokio::spawn(async move {
                            let reader = TokioBufReader::new(stderr);
                            let mut lines = reader.lines();
                            
                            while let Ok(Some(line)) = lines.next_line().await {
                                let log_entry = format!("[{}] RUN#{} STDERR: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_num, line);
                                let _ = SAFE_LOGGER.safe_append_log(&log_file_path, &log_entry);
                                eprintln!("[{}] Run#{} ERROR: {}", process_id, run_num, line);
                            }
                        });
                    }
                    
                    // Wait for the process to complete
                    match child.wait().await {
                        Ok(status) => {
                            let end_log = format!("[{}] SYSTEM {}:{}: Run #{} completed with status: {}\n", 
                                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, run_count, status);
                            let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &end_log);
                        }
                        Err(e) => {
                            let error_log = format!("[{}] SYSTEM {}:{}: Run #{} failed: {}\n", 
                                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, run_count, e);
                            let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &error_log);
                        }
                    }
                }
                Err(e) => {
                    let error_log = format!("[{}] SYSTEM {}:{}: Failed to start run #{}: {}\n", 
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, run_count, e);
                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &error_log);
                }
            }
            
            // Update status to waiting
            {
                let mut processes_lock = processes_clone.lock().unwrap();
                if let Some(managed_process) = processes_lock.get_mut(&config_clone.id) {
                    managed_process.status = ProcessStatus::WaitingForPeriod;
                }
            }
            
            // Wait for the next period
            tokio::time::sleep(Duration::from_secs(period_seconds)).await;
        }
    });
    
    let managed_process = ManagedProcess {
        config: config.clone(),
        child: None,
        log_file,
        stdin_sender: None,
        run_count: 0,
        last_run: None,
        periodic_handle: Some(periodic_handle),
        status: ProcessStatus::WaitingForPeriod,
        auto_restart_handle: None,
        last_exit_time: None,
    };
    
    {
        let mut processes_lock = processes.lock().unwrap();
        processes_lock.insert(config.id.clone(), managed_process);
    }
    
    println!("Started periodic process: {} (every {}s)", config.id, period_seconds);
}

async fn handle_add_process(
    req: AddProcessRequest,
    processes: ProcessMap,
    host: Arc<String>,
    port: u16,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Validate that the ID is unique
    {
        let processes_lock = processes.lock().unwrap();
        if processes_lock.contains_key(&req.id) {
            let response = ApiResponse {
                success: false,
                message: format!("Process with ID '{}' already exists. Please use a unique ID.", req.id),
            };
            return Ok(warp::reply::json(&response));
        }
    }
    
    // Validate ID is not empty or just whitespace
    if req.id.trim().is_empty() {
        let response = ApiResponse {
            success: false,
            message: "Process ID cannot be empty or just whitespace".to_string(),
        };
        return Ok(warp::reply::json(&response));
    }
    
    // Validate command is not empty or just whitespace
    if req.command.trim().is_empty() {
        let response = ApiResponse {
            success: false,
            message: "Process command cannot be empty or just whitespace".to_string(),
        };
        return Ok(warp::reply::json(&response));
    }
    
    let log_path = get_process_log_path(&req.id);
    
    let config = ProcessConfig {
        id: req.id.clone(),
        command: req.command,
        args: req.args,
        working_dir: req.working_dir,
        auto_restart: req.auto_restart,
        log_file: log_path.to_string_lossy().into_owned(),
        periodic: req.periodic.unwrap_or(false),
        period_seconds: req.period_seconds,
    };
    
    // Validate periodic configuration
    if config.periodic {
    if let Some(seconds) = config.period_seconds {
        if seconds <= 0 {
            let response = ApiResponse {
                success: false,
                message: "period_seconds must be greater than zero".to_string(),
            };
            return Ok(warp::reply::json(&response));
        }
    } else {
        let response = ApiResponse {
            success: false,
            message: "Periodic processes must specify period_seconds".to_string(),
        };
        return Ok(warp::reply::json(&response));
    }
}

    
    // Save to configuration file if requested
    if req.save_for_next_run {
        if let Err(e) = CONFIG_MANAGER.save_process_config(&config) {
            eprintln!("Failed to save process config: {}", e);
        }
    }
    
    start_process(processes, config.clone(), host, port).await;
    
    let process_type = if req.periodic.unwrap_or(false) {
        format!("periodic ({}s)", req.period_seconds.unwrap_or(0))
    } else {
        "regular".to_string()
    };
    
    let auto_restart_info = if config.auto_restart {
        " with auto-restart enabled"
    } else {
        ""
    };
    
    let response = ApiResponse {
        success: true,
        message: format!("Process {} added successfully as {}{}", req.id, process_type, auto_restart_info),
    };
    
    Ok(warp::reply::json(&response))
}

async fn handle_restart_process(
    id: String,
    processes: ProcessMap,
    host: Arc<String>,
    port: u16,
) -> Result<impl warp::Reply, warp::Rejection> {
    let config = {
        let mut processes_lock = processes.lock().unwrap();
        if let Some(managed_process) = processes_lock.get_mut(&id) {
            // Stop auto-restart monitor first
            if let Some(handle) = managed_process.auto_restart_handle.take() {
                handle.abort();
            }
            
            // Stop the current process
            if let Some(ref mut child) = managed_process.child {
                let _ = child.kill();
                let _ = child.wait();
            }
            
            // Stop periodic task if it exists
            if let Some(handle) = managed_process.periodic_handle.take() {
                handle.abort();
            }
            
            managed_process.child = None;
            managed_process.periodic_handle = None;
            managed_process.status = ProcessStatus::Stopped;
            Some(managed_process.config.clone())
        } else {
            None
        }
    };
    
    if let Some(config) = config {
        // Increment run count for manual restart
        {
            let mut processes_lock = processes.lock().unwrap();
            if let Some(managed_process) = processes_lock.get_mut(&id) {
                managed_process.run_count += 1;
            }
        }
        
        start_process(processes, config, host, port).await;
        let response = ApiResponse {
            success: true,
            message: format!("Process {} restarted successfully", id),
        };
        Ok(warp::reply::json(&response))
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_stop_process(
    id: String,
    processes: ProcessMap,
    host: Arc<String>,
    port: u16,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut processes_lock = processes.lock().unwrap();
    if let Some(managed_process) = processes_lock.get_mut(&id) {
        // Stop auto-restart monitor first
        if let Some(handle) = managed_process.auto_restart_handle.take() {
            handle.abort();
        }
        
        if let Some(ref mut child) = managed_process.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        
        // Stop periodic task if it exists
        if let Some(handle) = managed_process.periodic_handle.take() {
            handle.abort();
        }
        
        managed_process.child = None;
        managed_process.periodic_handle = None;
        managed_process.status = ProcessStatus::ManuallyStopped; // Mark as manually stopped
        
        // Log the stop
        let stop_log = format!("[{}] SYSTEM {}:{}: Process stopped manually via API\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port);
        let _ = SAFE_LOGGER.safe_append_log(&managed_process.config.log_file, &stop_log);
        
        let response = ApiResponse {
            success: true,
            message: format!("Process {} stopped successfully", id),
        };
        Ok(warp::reply::json(&response))
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_remove_process(
    id: String,
    processes: ProcessMap,
    host: Arc<String>,
    port: u16,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut processes_lock = processes.lock().unwrap();
    if let Some(mut managed_process) = processes_lock.remove(&id) {
        // Stop auto-restart monitor first
        if let Some(handle) = managed_process.auto_restart_handle.take() {
            handle.abort();
        }
        
        if let Some(ref mut child) = managed_process.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        
        // Stop periodic task if it exists
        if let Some(handle) = managed_process.periodic_handle.take() {
            handle.abort();
        }
        
        // Log the removal
        let remove_log = format!("[{}] SYSTEM {}:{}: Process removed from supervisor\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port);
        let _ = SAFE_LOGGER.safe_append_log(&managed_process.config.log_file, &remove_log);
        
        // Remove from saved configuration
        if let Err(e) = CONFIG_MANAGER.remove_process_config(&id) {
            eprintln!("Failed to remove process config: {}", e);
        }
        
        let response = ApiResponse {
            success: true,
            message: format!("Process {} removed successfully", id),
        };
        Ok(warp::reply::json(&response))
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_restart_all(processes: ProcessMap, host: Arc<String>, port: u16) -> Result<impl warp::Reply, warp::Rejection> {
    let configs: Vec<ProcessConfig> = {
        let mut processes_lock = processes.lock().unwrap();
        let mut configs = Vec::new();
        
        for (_, managed_process) in processes_lock.iter_mut() {
            // Stop auto-restart monitor first
            if let Some(handle) = managed_process.auto_restart_handle.take() {
                handle.abort();
            }
            
            // Stop regular processes
            if let Some(ref mut child) = managed_process.child {
                let _ = child.kill();
                let _ = child.wait();
            }
            
            // Stop periodic task if it exists
            if let Some(handle) = managed_process.periodic_handle.take() {
                handle.abort();
            }
            
            managed_process.child = None;
            managed_process.periodic_handle = None;
            managed_process.status = ProcessStatus::Stopped;
            configs.push(managed_process.config.clone());
        }
        configs
    };
    
    // Restart all processes
    for config in configs {
        start_process(processes.clone(), config, host.clone(), port).await;
    }
    
    let response = ApiResponse {
        success: true,
        message: "All processes restarted successfully".to_string(),
    };
    Ok(warp::reply::json(&response))
}

async fn handle_stop_all(processes: ProcessMap) -> Result<impl warp::Reply, warp::Rejection> {
    let mut processes_lock = processes.lock().unwrap();
    for (_, managed_process) in processes_lock.iter_mut() {
        // Stop auto-restart monitor first
        if let Some(handle) = managed_process.auto_restart_handle.take() {
            handle.abort();
        }
        
        // Stop regular processes
        if let Some(ref mut child) = managed_process.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        
        // Stop periodic task if it exists
        if let Some(handle) = managed_process.periodic_handle.take() {
            handle.abort();
        }
        
        managed_process.child = None;
        managed_process.periodic_handle = None;
        managed_process.status = ProcessStatus::ManuallyStopped; // Mark as manually stopped
    }
    
    let response = ApiResponse {
        success: true,
        message: "All processes stopped successfully".to_string(),
    };
    Ok(warp::reply::json(&response))
}

async fn handle_send_input(
    id: String,
    req: ProcessInputRequest,
    processes: ProcessMap,
) -> Result<impl warp::Reply, warp::Rejection> {
    let processes_lock = processes.lock().unwrap();
    if let Some(managed_process) = processes_lock.get(&id) {
        if let Some(ref sender) = managed_process.stdin_sender {
            match sender.send(req.input) {
                Ok(_) => {
                    let response = ApiResponse {
                        success: true,
                        message: format!("Input sent to process {}", id),
                    };
                    Ok(warp::reply::json(&response))
                }
                Err(e) => {
                    let response = ApiResponse {
                        success: false,
                        message: format!("Failed to send input to process {}: {}", id, e),
                    };
                    Ok(warp::reply::json(&response))
                }
            }
        } else {
            let response = ApiResponse {
                success: false,
                message: format!("Process {} has no stdin channel or is periodic", id),
            };
            Ok(warp::reply::json(&response))
        }
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_clear_log(
    id: String,
    processes: ProcessMap,
) -> Result<impl warp::Reply, warp::Rejection> {
    let processes_lock = processes.lock().unwrap();
    if let Some(managed_process) = processes_lock.get(&id) {
        match std::fs::write(&managed_process.config.log_file, "") {
            Ok(_) => {
                let response = ApiResponse {
                    success: true,
                    message: format!("Log cleared for process {}", id),
                };
                Ok(warp::reply::json(&response))
            }
            Err(e) => {
                let response = ApiResponse {
                    success: false,
                    message: format!("Failed to clear log for process {}: {}", id, e),
                };
                Ok(warp::reply::json(&response))
            }
        }
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_list_processes(processes: ProcessMap) -> Result<impl warp::Reply, warp::Rejection> {
    let processes_lock = processes.lock().unwrap();
    let mut process_list = Vec::new();
    
    for (id, managed_process) in processes_lock.iter() {
        let is_running = managed_process.child.is_some() || managed_process.periodic_handle.is_some();
        let status_str = match &managed_process.status {
            ProcessStatus::Running => "running",
            ProcessStatus::Stopped => "stopped",
            ProcessStatus::WaitingForPeriod => "waiting",
            ProcessStatus::Failed => "failed",
            ProcessStatus::ManuallyStopped => "manually_stopped",
        };
        
        process_list.push(serde_json::json!({
            "id": id,
            "command": managed_process.config.command,
            "args": managed_process.config.args,
            "status": status_str,
            "is_running": is_running,
            "log_file": managed_process.config.log_file,
            "auto_restart": managed_process.config.auto_restart,
            "periodic": managed_process.config.periodic,
            "period_seconds": managed_process.config.period_seconds,
            "run_count": managed_process.run_count,
            "last_run": managed_process.last_run
        }));
    }
    
    Ok(warp::reply::json(&process_list))
}

async fn handle_exeio_info(host: Arc<String>, port: u16) -> Result<impl warp::Reply, warp::Rejection> {
    let info = serde_json::json!({
        "name": "exeio - Process Supervisor",
        "version": "1.0.0",
        "author": "made by philo",
        "url": format!("http://{}:{}", host, port),
        "endpoints": [
            "POST /add - Add new process",
            "POST /restart/:id - Restart process",
            "POST /stop/:id - Stop process",
            "POST /remove/:id - Remove process",
            "POST /restart-all - Restart all processes",
            "POST /stop-all - Stop all processes",
            "POST /input/:id - Send input to process",
            "POST /clear-log/:id - Clear process log",
            "GET /list - List all processes",
            "GET /info - Get supervisor information",
            "GET /logs/:id?page=1&page_size=50 - Get paginated process logs",
            "POST /shutdown - Shutdown supervisor"
        ]
    });
    
    Ok(warp::reply::json(&info))
}

async fn handle_process_logs(
    id: String,
    params: PaginationParams,
    processes: ProcessMap
) -> Result<impl warp::Reply, warp::Rejection> {
    let processes_lock = processes.lock().unwrap();
    if let Some(managed_process) = processes_lock.get(&id) {
        let log_file_path = &managed_process.config.log_file;
        let page = params.page.unwrap_or(1).max(1); 
        let page_size = params.page_size.unwrap_or(50).max(1);

        match read_logs_reverse_paginated(log_file_path, page, page_size) {
            Ok((logs, total_lines)) => {
                let response = serde_json::json!({
                    "success": true,
                    "page": page,
                    "page_size": page_size,
                    "total_lines": total_lines,
                    "logs": logs
                });
                Ok(warp::reply::json(&response))
            }
            Err(e) => {
                let response = ApiResponse {
                    success: false,
                    message: format!("Failed to read logs for process {}: {}", id, e),
                };
                Ok(warp::reply::json(&response))
            }
        }
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

async fn handle_shutdown(
    processes: ProcessMap,
    host: Arc<String>,
    port: u16,
) -> Result<impl warp::Reply, warp::Rejection> {
    // Log shutdown request
    let shutdown_log = format!("[{}] SYSTEM {}:{}: Shutdown requested via API\n", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port);
    log_exeio_event(&shutdown_log, &host, port);
    
    // Stop all processes gracefully
    {
        let mut processes_lock = processes.lock().unwrap();
        for (id, managed_process) in processes_lock.iter_mut() {
            // Stop auto-restart monitor first
            if let Some(handle) = managed_process.auto_restart_handle.take() {
                println!("Stopping auto-restart monitor for: {}", id);
                handle.abort();
            }
            
            // Stop regular processes
            if let Some(ref mut child) = managed_process.child {
                println!("Stopping process: {}", id);
                let _ = child.kill();
                let _ = child.wait();
            }
            
            // Stop periodic tasks
            if let Some(handle) = managed_process.periodic_handle.take() {
                println!("Stopping periodic task: {}", id);
                handle.abort();
            }
            
            // Log process stop
            let stop_log = format!("[{}] SYSTEM {}:{}: Process stopped due to supervisor shutdown\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port);
            let _ = SAFE_LOGGER.safe_append_log(&managed_process.config.log_file, &stop_log);
            
            managed_process.child = None;
            managed_process.periodic_handle = None;
            managed_process.status = ProcessStatus::Stopped;
        }
    }
    
    // Log final shutdown message
    let final_log = format!("[{}] SYSTEM {}:{}: exeio supervisor shutting down\n", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host, port);
    log_exeio_event(&final_log, &host, port);
    
    println!("Shutting down exeio supervisor...");
    
    let response = ApiResponse {
        success: true,
        message: "Supervisor shutdown initiated".to_string(),
    };
    
    // Schedule shutdown after a brief delay to allow response to be sent
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cleanup_lock_file(&get_lock_file_path());
        std::process::exit(0);
    });
    
    Ok(warp::reply::json(&response))
}

fn get_config_path() -> std::path::PathBuf {
    let mut config_dir = dirs::home_dir().unwrap_or_else(|| {
        eprintln!("Could not determine home directory, using current directory instead");
        std::env::current_dir().unwrap_or_default()
    });
    
    config_dir.push(".config");
    config_dir.push("exeio");
    
    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&config_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create config directory: {}", e);
    });
    
    config_dir.push("processes.json");
    config_dir
}

fn get_logs_dir() -> PathBuf {
    let mut logs_dir = dirs::home_dir().unwrap_or_else(|| {
        eprintln!("Could not determine home directory, using current directory instead");
        std::env::current_dir().unwrap_or_default()
    });
    
    logs_dir.push(".local");
    logs_dir.push("share");
    logs_dir.push("exeio");
    logs_dir.push("logs");
    
    // Create the directory if it doesn't exist
    std::fs::create_dir_all(&logs_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create logs directory: {}", e);
    });
    
    logs_dir
}

fn get_process_log_path(process_id: &str) -> PathBuf {
    let mut path = get_logs_dir();
    path.push(format!("{}.log", process_id));
    path
}

// Update function signature to use &str instead of String
fn init_exeio_log(host: &Arc<String>, port: u16) -> PathBuf {
    let mut log_path = get_logs_dir();
    log_path.push("exeio.log");
    
    // Log startup information
    let start_log = format!("[{}] SYSTEM {}:{}: exeio process supervisor started\n", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), 
        host, port
    );
    
    let _ = SAFE_LOGGER.safe_append_log(&log_path.to_string_lossy(), &start_log);
    log_path
}

// Update function signature to accept Arc<String>
fn log_exeio_event(event: &str, host: &Arc<String>, port: u16) {
    let log_path = get_logs_dir().join("exeio.log");
    let log_entry = format!("[{}] SYSTEM {}:{}: {}", 
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), 
        host, port, event);
    
    let _ = SAFE_LOGGER.safe_append_log(&log_path.to_string_lossy(), &log_entry);
}

fn ensure_single_instance() -> Result<(), String> {
    let lock_path = get_lock_file_path();
    
    // Check if lock file exists and if the process is still running
    if lock_path.exists() {
        if let Ok(pid_content) = fs::read_to_string(&lock_path) {
            if let Ok(pid) = pid_content.trim().parse::<u32>() {
                if is_process_running(pid) {
                    return Err(format!(
                        "Another instance of exeio is already running (PID: {}). Lock file: {}",
                        pid, lock_path.display()
                    ));
                } else {
                    // Process not running, remove stale lock file
                    let _ = fs::remove_file(&lock_path);
                }
            }
        }
    }
    
    // Create lock file with current PID atomically using temp file + rename
    let current_pid = std::process::id();
    let temp_lock_path = lock_path.with_extension("lock.tmp");
    
    if let Err(e) = fs::write(&temp_lock_path, current_pid.to_string()) {
        return Err(format!("Failed to create temp lock file: {}", e));
    }
    
    if let Err(e) = fs::rename(&temp_lock_path, &lock_path) {
        let _ = fs::remove_file(&temp_lock_path); // Clean up temp file
        return Err(format!("Failed to create lock file atomically: {}", e));
    }
    
    // Set up cleanup handler for graceful shutdown
    setup_cleanup_handler(lock_path.clone());
    
    println!("Single instance lock acquired (PID: {}, Lock: {})", current_pid, lock_path.display());
    Ok(())
}

fn get_lock_file_path() -> PathBuf {
    let mut lock_dir = dirs::home_dir().unwrap_or_else(|| {
        eprintln!("Could not determine home directory, using /tmp instead");
        PathBuf::from("/tmp")
    });
    
    lock_dir.push(".local");
    lock_dir.push("share");
    lock_dir.push("exeio");
    
    // Create the directory if it doesn't exist
    fs::create_dir_all(&lock_dir).unwrap_or_else(|e| {
        eprintln!("Failed to create lock directory: {}", e);
    });
    
    lock_dir.push("exeio.lock");
    lock_dir
}

fn is_process_running(pid: u32) -> bool {
 
        use std::process::Command;
        
        // Use kill -0 to check if process exists without actually killing it
        match Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
        {
            Ok(output) => {
                let result = output.status.success();
                eprintln!("DEBUG: kill -0 {} -> exit_code: {:?}, success: {}", 
                         pid, output.status.code(), result);
                result
            },
            Err(e) => {
                eprintln!("DEBUG: kill -0 {} -> error: {}", pid, e);
                false
            },
        }

}

fn setup_cleanup_handler(lock_path: PathBuf) {
    // Set up signal handlers for graceful cleanup
    #[cfg(unix)]
    {
        use signal_hook::{consts::SIGTERM, consts::SIGINT, iterator::Signals};
        
        let lock_path_clone = lock_path.clone();
        thread::spawn(move || {
            let mut signals = Signals::new(&[SIGINT, SIGTERM]).expect("Failed to create signal handler");
            for _sig in signals.forever() {
                cleanup_lock_file(&lock_path_clone);
                std::process::exit(0);
            }
        });
    }
    
    // Also clean up on normal program termination
    let lock_path_clone = lock_path;
    std::panic::set_hook(Box::new(move |_| {
        cleanup_lock_file(&lock_path_clone);
    }));
}

fn cleanup_lock_file(lock_path: &PathBuf) {
    if lock_path.exists() {
        if let Err(e) = fs::remove_file(lock_path) {
            eprintln!("Failed to remove lock file: {}", e);
        } else {
            println!("Lock file removed: {}", lock_path.display());
        }
    }
}

fn read_logs_reverse_paginated(
    log_file_path: &str, 
    page: usize, 
    page_size: usize
) -> Result<(Vec<String>, usize), std::io::Error> {
    let file = File::open(log_file_path)?;
    let file_size = file.metadata()?.len();
    
    if file_size == 0 {
        return Ok((Vec::new(), 0));
    }
    
    // For small files (< 512KB) or early pages, use simpler approach
    if file_size < 524_288 || page <= 3 {
        return read_logs_simple(file, page, page_size);
    }
    
    // For large files, use chunked reading
    read_logs_chunked(file, file_size, page, page_size)
}

fn read_logs_simple(
    file: File, 
    page: usize, 
    page_size: usize
) -> Result<(Vec<String>, usize), std::io::Error> {
    let mut reader = BufReader::with_capacity(32768, file);
    let mut all_lines = Vec::new();
    let mut line = String::new();
    
    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            all_lines.push(trimmed.to_string());
        }
        line.clear();
    }
    
    let total_lines = all_lines.len();
    all_lines.reverse(); // Reverse to get newest first
    
    let lines_to_skip = (page - 1) * page_size;
    let result_lines: Vec<String> = all_lines
        .into_iter()
        .skip(lines_to_skip)
        .take(page_size)
        .collect();
    
    Ok((result_lines, total_lines))
}

fn read_logs_chunked(
    file: File,
    file_size: u64,
    page: usize,
    page_size: usize,
) -> Result<(Vec<String>, usize), std::io::Error> {
    use std::io::{Seek, SeekFrom};
    
    let mut reader = BufReader::with_capacity(32768, file);
    let mut lines = Vec::new();
    let mut total_lines = 0;
    let mut buffer = Vec::new();
    let mut pos = file_size;
    
    let lines_to_skip = (page - 1) * page_size;
    let lines_to_read = page_size;
    let mut lines_found = 0;
    let mut lines_collected = 0;
    
    const CHUNK_SIZE: u64 = 32768; // 32KB chunks
    
    while pos > 0 && (lines_collected < lines_to_read || total_lines == 0) {
        let chunk_start = if pos >= CHUNK_SIZE { pos - CHUNK_SIZE } else { 0 };
        let chunk_size = pos - chunk_start;
        
        reader.seek(SeekFrom::Start(chunk_start))?;
        
        let mut chunk = vec![0u8; chunk_size as usize];
        reader.read_exact(&mut chunk)?;
        
        if buffer.is_empty() {
            buffer = chunk;
        } else {
            let mut new_buffer = Vec::with_capacity(chunk.len() + buffer.len());
            new_buffer.extend_from_slice(&chunk);
            new_buffer.extend_from_slice(&buffer);
            buffer = new_buffer;
        }
        
        let mut line_start = buffer.len();
        
        // Scan backwards for newlines
        for (i, &byte) in buffer.iter().enumerate().rev() {
            if byte == b'\n' || i == 0 {
                let line_end = if byte == b'\n' { line_start } else { buffer.len() };
                let actual_start = if byte == b'\n' && i > 0 { i + 1 } else { i };
                
                if actual_start < line_end {
                    let line = String::from_utf8_lossy(&buffer[actual_start..line_end]);
                    let trimmed_line = line.trim_end();
                    if !trimmed_line.is_empty() {
                        total_lines += 1;
                        lines_found += 1;
                        
                        // Only collect lines we want to return (after skipping)
                        if lines_found > lines_to_skip && lines_collected < lines_to_read {
                            lines.push(trimmed_line.to_string());
                            lines_collected += 1;
                        }
                    }
                }
                line_start = i;
            }
        }
        
        // Keep only the unprocessed part of buffer for next iteration
        if line_start > 0 {
            buffer.truncate(line_start);
        } else {
            buffer.clear();
        }
        
        pos = chunk_start;
        
        // Early termination if we have collected enough lines and read the whole file
        if lines_collected >= lines_to_read && pos == 0 {
            break;
        }
    }
    
    lines.reverse();
    Ok((lines, total_lines))
}

fn start_auto_restart_monitor(
    processes: ProcessMap, 
    config: ProcessConfig, 
    host: Arc<String>, 
    port: u16, 
    child_pid: u32
) -> tokio::task::JoinHandle<()> {
    let processes_clone = processes.clone();
    let config_clone = config.clone();
    let host_clone = host.clone();
    
    tokio::spawn(async move {
        // Log that monitoring started
        let monitor_log = format!("[{}] SYSTEM {}:{}: Auto-restart monitor started for process '{}' (PID: {})\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid);
        let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &monitor_log);
        
        // Get the child process handle and wait for it asynchronously
        let child_option = {
            let mut processes_lock = processes_clone.lock().unwrap();
            if let Some(managed_process) = processes_lock.get_mut(&config_clone.id) {
                managed_process.child.take()
            } else {
                None
            }
        };
        
        if let Some(mut child) = child_option {
            // Wait for the child process to exit efficiently using tokio's async wait
            let wait_result = tokio::task::spawn_blocking(move || {
                child.wait()
            }).await;
            
            match wait_result {
                Ok(Ok(exit_status)) => {
                    let now = chrono::Utc::now();
                    
                    // Log process exit detection
                    let exit_log = format!("[{}] SYSTEM {}:{}: Auto-restart monitor detected process '{}' (PID: {}) has exited with status: {}\n", 
                        now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid, exit_status);
                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &exit_log);
                    
                    // Check if we should restart the process
                    let (should_restart, restart_delay, was_manual_stop) = {
                        let mut processes_lock = processes_clone.lock().unwrap();
                        if let Some(managed_process) = processes_lock.get_mut(&config_clone.id) {
                            managed_process.last_exit_time = Some(now);
                            
                            // Check the current status to determine if this was a manual stop
                            let was_manual = matches!(managed_process.status, ProcessStatus::ManuallyStopped);
                            
                            // Only restart if the process is still marked as Running (not manually stopped)
                            let should_restart = matches!(managed_process.status, ProcessStatus::Running) && 
                                                 config_clone.auto_restart;
                            
                            if should_restart {
                                // Update status to Failed for restart
                                managed_process.status = ProcessStatus::Failed;
                                managed_process.child = None;
                                managed_process.stdin_sender = None;
                                
                                // Calculate restart delay with exponential backoff to prevent restart loops
                                let delay = calculate_restart_delay(managed_process.run_count, managed_process.last_exit_time);
                                
                                // Increment run count for restart
                                managed_process.run_count += 1;
                                
                                (true, delay, was_manual)
                            } else {
                                // Process was manually stopped or auto-restart is disabled
                                managed_process.child = None;
                                managed_process.stdin_sender = None;
                                if !was_manual {
                                    managed_process.status = ProcessStatus::Stopped;
                                }
                                (false, 0, was_manual)
                            }
                        } else {
                            (false, 0, false)
                        }
                    };
                    
                    // Log restart decision
                    let decision_log = format!("[{}] SYSTEM {}:{}: Auto-restart decision for '{}': should_restart={}, was_manual_stop={}, delay={}s\n", 
                        now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, should_restart, was_manual_stop, restart_delay);
                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &decision_log);
                    
                    if should_restart {
                        // Log restart initiation
                        let restart_init_log = format!("[{}] SYSTEM {}:{}: Initiating auto-restart for process '{}' (PID: {}) in {}s\n", 
                            now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid, restart_delay);
                        let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &restart_init_log);
                        
                        // Send restart request with delay
                        if let Some(sender) = RESTART_SENDER.lock().unwrap().as_ref() {
                            let exit_code = exit_status.code().unwrap_or(-1);
                            let reason = if exit_code == 0 {
                                format!("Auto-restarting process '{}' after normal exit (PID: {})", config_clone.id, child_pid)
                            } else if exit_code == 9 || exit_code == 15 {
                                format!("Auto-restarting process '{}' after external kill signal {} (PID: {})", config_clone.id, exit_code, child_pid)
                            } else {
                                format!("Auto-restarting process '{}' after crash with exit code {} (PID: {})", config_clone.id, exit_code, child_pid)
                            };
                            
                            let request = RestartRequest {
                                process_id: config_clone.id.clone(),
                                delay_seconds: restart_delay,
                                reason,
                            };
                            
                            match sender.send(request) {
                                Ok(_) => {
                                    let send_log = format!("[{}] SYSTEM {}:{}: Auto-restart request sent for process '{}'\n", 
                                        now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id);
                                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &send_log);
                                }
                                Err(e) => {
                                    let error_log = format!("[{}] SYSTEM {}:{}: Failed to send auto-restart request for process '{}': {}\n", 
                                        now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, e);
                                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &error_log);
                                }
                            }
                        }
                    } else if was_manual_stop {
                        let manual_log = format!("[{}] SYSTEM {}:{}: Process '{}' (PID: {}) exited after manual stop - no restart\n", 
                            now.format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid);
                        let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &manual_log);
                    }
                }
                Ok(Err(e)) => {
                    let error_log = format!("[{}] SYSTEM {}:{}: Error waiting for process '{}' (PID: {}): {}\n", 
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid, e);
                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &error_log);
                }
                Err(e) => {
                    let error_log = format!("[{}] SYSTEM {}:{}: Tokio task error for process '{}' (PID: {}): {}\n", 
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid, e);
                    let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &error_log);
                }
            }
        } else {
            let no_child_log = format!("[{}] SYSTEM {}:{}: No child process found for auto-restart monitoring of '{}' (PID: {})\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid);
            let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &no_child_log);
        }
        
        // Clean up auto-restart handle when monitor ends
        {
            let mut processes_lock = processes_clone.lock().unwrap();
            if let Some(managed_process) = processes_lock.get_mut(&config_clone.id) {
                managed_process.auto_restart_handle = None;
            }
        }
        
        // Log that monitoring ended
        let end_log = format!("[{}] SYSTEM {}:{}: Auto-restart monitor ended for process '{}' (PID: {})\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), host_clone, port, config_clone.id, child_pid);
        let _ = SAFE_LOGGER.safe_append_log(&config_clone.log_file, &end_log);
    })
}

// Calculate restart delay with exponential backoff and recent exit detection
fn calculate_restart_delay(run_count: u64, last_exit_time: Option<chrono::DateTime<chrono::Utc>>) -> u64 {
    // If the process exited very recently (within 10 seconds), increase delay significantly
    let rapid_restart_penalty = if let Some(last_exit) = last_exit_time {
        let time_since_last_exit = chrono::Utc::now().signed_duration_since(last_exit);
        if time_since_last_exit.num_seconds() < 10 {
            20 // Add 20 seconds penalty for rapid restarts
        } else {
            0
        }
    } else {
        0
    };
    
    // Base delay calculation with exponential backoff
    let base_delay = if run_count <= 3 {
        2 // 2 seconds for first 3 restarts
    } else if run_count <= 6 {
        5 // 5 seconds for next 3 restarts
    } else if run_count <= 10 {
        15 // 15 seconds for next 4 restarts
    } else if run_count <= 15 {
        30 // 30 seconds for next 5 restarts
    } else {
        60 // 1 minute for subsequent restarts
    };
    
    base_delay + rapid_restart_penalty
}


