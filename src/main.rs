use std::process::Command;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::process::Command as TokioCommand;
use warp::Filter;

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
}

#[derive(Debug, Clone, Serialize)]
enum ProcessStatus {
    Running,
    Stopped,
    WaitingForPeriod,
    Failed,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let processes: ProcessMap = Arc::new(Mutex::new(HashMap::new()));
    let host = Arc::new(cli.host.clone());
    
    let log_filter = warp::log::custom(|info| {
        println!(
            "exieo: [{}] {} {} -> {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
            info.method(),
            info.path(),
            info.status()
        );
    });
    
    load_and_start_processes(processes.clone()).await;
    
    // Setup API routes
    let processes_filter = warp::any().map(move || processes.clone());
    
    let add_process = warp::path("add")
        .and(warp::post())
        .and(warp::body::json())
        .and(processes_filter.clone())
        .and_then(handle_add_process);
    
    let restart_process = warp::path("restart")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_restart_process);
    
    let stop_process = warp::path("stop")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_stop_process);
    
    let remove_process = warp::path("remove")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_remove_process);
    
    let restart_all = warp::path("restart-all")
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_restart_all);
    
    let stop_all = warp::path("stop-all")
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_stop_all);
    
    let send_input = warp::path("input")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(warp::body::json())
        .and(processes_filter.clone())
        .and_then(handle_send_input);
    
    let clear_log = warp::path("clear-log")
        .and(warp::path::param::<String>())
        .and(warp::post())
        .and(processes_filter.clone())
        .and_then(handle_clear_log);
    
    let list_processes = warp::path("list")
        .and(warp::get())
        .and(processes_filter.clone())
        .and_then(handle_list_processes);

    let exeio_info = warp::path("info")
        .and(warp::get())
        .and(warp::any().map(move || host.as_str().to_string()))
        .and(warp::any().map(move || cli.port))
        .and_then(handle_exeio_info);
    
    let logs_route = warp::path("logs")
        .and(warp::path::param::<String>())
        .and(warp::get())
        .and(warp::query::<PaginationParams>())
        .and(processes_filter.clone())
        .and_then(handle_process_logs);
    
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
        .with(log_filter)
        .with(warp::cors().allow_any_origin());
   
    println!("Process Supervisor starting on port {} at {}" , cli.port , cli.host);
    println!("Available endpoints:");
    println!("  POST /add - Add new process");
    println!("  POST /restart/:id - Restart process");
    println!("  POST /stop/:id - Stop process");
    println!("  POST /remove/:id - Remove process");
    println!("  POST /restart-all - Restart all processes");
    println!("  POST /stop-all - Stop all processes");
    println!("  POST /input/:id - Send input to process");
    println!("  POST /clear-log/:id - Clear process log");
    println!("  GET /list - List all processes");
    println!("  GET /info - Get supervisor information");
    println!("GET /logs/:id?page=1&page_size=50 - Get paginated process logs")
    
    let addr: std::net::IpAddr = cli.host.parse()
    .unwrap_or_else(|_| {
        eprintln!("Invalid host address: {}", cli.host);
        std::process::exit(1);
    });
    warp::serve(routes)
        .run((addr, cli.port))
        .await;
}

async fn load_and_start_processes(processes: ProcessMap) {
    // Try to load configuration from file
    if let Ok(config_content) = std::fs::read_to_string("processes.json") {
        // from string to process configurations struct
        if let Ok(configs) = serde_json::from_str::<Vec<ProcessConfig>>(&config_content) {
            for config in configs {
                start_process(processes.clone(), config).await;
            }
        }
    }
}

async fn start_process(processes: ProcessMap, config: ProcessConfig) {
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
    
    // Log process start
    if let Ok(mut file) = OpenOptions::new().append(true).open(&config.log_file) {
        let start_log = format!("[{}] SYSTEM: Starting process '{}' (Run #1)\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), config.id);
        let _ = file.write_all(start_log.as_bytes());
    }
    
    if config.periodic && config.period_seconds.is_some() {
        start_periodic_process(processes, config, log_file).await;
    } else {
        start_regular_process(processes, config, log_file).await;
    }
}

async fn start_regular_process(processes: ProcessMap, config: ProcessConfig, log_file: File) {
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
            // create a channel for sending input to the child process
            let (stdin_sender, stdin_receiver) = std::sync::mpsc::channel::<String>();
            
            // Handle stdin , takes the Option
            if let Some(stdin) = child.stdin.take() {
                let mut stdin = stdin;
                thread::spawn(move || {
                    // waits the sender
                    while let Ok(input) = stdin_receiver.recv() {
                        if let Err(e) = writeln!(stdin, "{}", input) {
                            eprintln!("Failed to write to process stdin: {}", e);
                            break;
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
                                if let Ok(mut file) = OpenOptions::new().append(true).open(&log_file_path) {
                                    let _ = file.write_all(log_entry.as_bytes());
                                }
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
                                if let Ok(mut file) = OpenOptions::new().append(true).open(&log_file_path) {
                                    let _ = file.write_all(log_entry.as_bytes());
                                }
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
                run_count: 1,
                last_run: Some(chrono::Utc::now()),
                periodic_handle: None,
                status: ProcessStatus::Running,
            };
            
            {
                let mut processes_lock = processes.lock().unwrap();
                processes_lock.insert(config.id.clone(), managed_process);
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
            };
            
            let mut processes_lock = processes.lock().unwrap();
            processes_lock.insert(config.id.clone(), managed_process);
        }
    }
}

async fn start_periodic_process(processes: ProcessMap, config: ProcessConfig, log_file: File) {
    let period_seconds = config.period_seconds.unwrap_or(60);
    let processes_clone = processes.clone();
    let config_clone = config.clone();
    
    let periodic_handle = tokio::spawn(async move {
        let mut run_count = 0u64;
        
        loop {
            run_count += 1;
            
            // Log periodic run start
            if let Ok(mut file) = OpenOptions::new().append(true).open(&config_clone.log_file) {
                let run_log = format!("[{}] SYSTEM: Starting periodic run #{} (every {}s)\n", 
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_count, period_seconds);
                let _ = file.write_all(run_log.as_bytes());
            }
            
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
                                if let Ok(mut file) = OpenOptions::new().append(true).open(&log_file_path) {
                                    let _ = file.write_all(log_entry.as_bytes());
                                }
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
                                if let Ok(mut file) = OpenOptions::new().append(true).open(&log_file_path) {
                                    let _ = file.write_all(log_entry.as_bytes());
                                }
                                eprintln!("[{}] Run#{} ERROR: {}", process_id, run_num, line);
                            }
                        });
                    }
                    
                    // Wait for the process to complete
                    match child.wait().await {
                        Ok(status) => {
                            if let Ok(mut file) = OpenOptions::new().append(true).open(&config_clone.log_file) {
                                let end_log = format!("[{}] SYSTEM: Run #{} completed with status: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_count, status);
                                let _ = file.write_all(end_log.as_bytes());
                            }
                        }
                        Err(e) => {
                            if let Ok(mut file) = OpenOptions::new().append(true).open(&config_clone.log_file) {
                                let error_log = format!("[{}] SYSTEM: Run #{} failed: {}\n", 
                                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_count, e);
                                let _ = file.write_all(error_log.as_bytes());
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut file) = OpenOptions::new().append(true).open(&config_clone.log_file) {
                        let error_log = format!("[{}] SYSTEM: Failed to start run #{}: {}\n", 
                            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), run_count, e);
                        let _ = file.write_all(error_log.as_bytes());
                    }
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
) -> Result<impl warp::Reply, warp::Rejection> {
    let config = ProcessConfig {
        id: req.id.clone(),
        command: req.command,
        args: req.args,
        working_dir: req.working_dir,
        auto_restart: req.auto_restart,
        log_file: format!("logs/{}.log", req.id),
        periodic: req.periodic.unwrap_or(false),
        period_seconds: req.period_seconds,
    };
    
    // Validate periodic configuration
    if config.periodic && config.period_seconds.is_none() {
        let response = ApiResponse {
            success: false,
            message: "Periodic processes must specify period_seconds".to_string(),
        };
        return Ok(warp::reply::json(&response));
    }
    
    // Create logs directory if it doesn't exist
    let _ = std::fs::create_dir_all("logs");
    
    // Save to configuration file if requested
    if req.save_for_next_run {
        save_process_config(&config);
    }
    
    start_process(processes, config).await;
    
    let process_type = if req.periodic.unwrap_or(false) {
        format!("periodic ({}s)", req.period_seconds.unwrap_or(0))
    } else {
        "regular".to_string()
    };
    
    let response = ApiResponse {
        success: true,
        message: format!("Process {} added successfully as {}", req.id, process_type),
    };
    
    Ok(warp::reply::json(&response))
}

async fn handle_restart_process(
    id: String,
    processes: ProcessMap,
) -> Result<impl warp::Reply, warp::Rejection> {
    let config = {
        let mut processes_lock = processes.lock().unwrap();
        if let Some(managed_process) = processes_lock.get_mut(&id) {
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
        start_process(processes, config).await;
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
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut processes_lock = processes.lock().unwrap();
    if let Some(managed_process) = processes_lock.get_mut(&id) {
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
        
        // Log the stop
        if let Ok(mut file) = OpenOptions::new().append(true).open(&managed_process.config.log_file) {
            let stop_log = format!("[{}] SYSTEM: Process stopped manually\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"));
            let _ = file.write_all(stop_log.as_bytes());
        }
        
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
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut processes_lock = processes.lock().unwrap();
    if let Some(mut managed_process) = processes_lock.remove(&id) {
        if let Some(ref mut child) = managed_process.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        
        // Stop periodic task if it exists
        if let Some(handle) = managed_process.periodic_handle.take() {
            handle.abort();
        }
        
        // Log the removal
        if let Ok(mut file) = OpenOptions::new().append(true).open(&managed_process.config.log_file) {
            let remove_log = format!("[{}] SYSTEM: Process removed from supervisor\n", 
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"));
            let _ = file.write_all(remove_log.as_bytes());
        }
        
        // Remove from saved configuration
        remove_process_config(&id);
        
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

async fn handle_restart_all(processes: ProcessMap) -> Result<impl warp::Reply, warp::Rejection> {
    let configs: Vec<ProcessConfig> = {
        let mut processes_lock = processes.lock().unwrap();
        let mut configs = Vec::new();
        
        for (_, managed_process) in processes_lock.iter_mut() {
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
        start_process(processes.clone(), config).await;
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

async fn handle_exeio_info(host: String, port: u16) -> Result<impl warp::Reply, warp::Rejection> {
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
            "GET /logs/:id?page=1&page_size=50 - Get paginated process logs"
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

        let lines = match std::fs::read_to_string(log_file_path) {
            Ok(content) => content.lines().map(|s| s.to_string()).collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };

        let total_lines = lines.len();
        let start = (page - 1) * page_size;
        let end = start + page_size;
        let snippet = if start < total_lines {
            lines[start.min(total_lines)..end.min(total_lines)].to_vec()
        } else {
            Vec::new()
        };

        let response = serde_json::json!({
            "success": true,
            "page": page,
            "page_size": page_size,
            "total_lines": total_lines,
            "logs": snippet
        });
        Ok(warp::reply::json(&response))
    } else {
        let response = ApiResponse {
            success: false,
            message: format!("Process {} not found", id),
        };
        Ok(warp::reply::json(&response))
    }
}

fn save_process_config(config: &ProcessConfig) {
    let mut configs = load_configs();
    configs.retain(|c| c.id != config.id); // Remove existing config with same id
    configs.push(config.clone());
    
    if let Ok(json) = serde_json::to_string_pretty(&configs) {
        let _ = std::fs::write("processes.json", json);
    }
}

fn remove_process_config(id: &str) {
    let mut configs = load_configs();
    configs.retain(|c| c.id != id);
    
    if let Ok(json) = serde_json::to_string_pretty(&configs) {
        let _ = std::fs::write("processes.json", json);
    }
}

fn load_configs() -> Vec<ProcessConfig> {
    if let Ok(content) = std::fs::read_to_string("processes.json") {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    }
}
