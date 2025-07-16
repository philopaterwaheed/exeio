# exeio


<img width="1024" height="1024" alt="exeio_logo" src="https://github.com/user-attachments/assets/36c337a0-fe6b-4504-83f6-8a21e065aa6f" />

A powerful process supervisor written in Rust that helps developers, system administrators, and DevOps engineers run, monitor, and manage processes remotely through a secure REST API interface.

> **Origin Story**: I developed exeio while managing my project [Planitly](https://github.com/philopaterwaheed/planitly_backend), where I needed to run and monitor multiple processes on server startup without SSH access. The ability to view logs, provide input when needed, and control processes through a simple API interface became essential for efficient server management.

## Table of Contents

- [Features](#features)
- [Use Cases & Applications](#use-cases--applications)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Auto-Restart Feature](#auto-restart-feature)
- [API Reference](#api-reference)
- [Examples](#examples)
- [Architecture](#architecture)
- [Security](#security)
- [Logging & Monitoring](#logging--monitoring)
- [Contributing](#contributing)
- [License](#license)

## Features

### Core Capabilities

- **Process Supervision:** Start, stop, restart, and monitor multiple processes
- **Auto-Restart:** Intelligent automatic process recovery with exponential backoff
- **REST API:** Complete HTTP-based control interface with authentication
- **Periodic Processes:** Schedule recurring tasks with configurable intervals
- **Real-time Monitoring:** Live process status tracking and logging
- **Comprehensive Logging:** Per-process and system-wide logging with timestamps
- **Security:** API key authentication for all operations
- **Persistence:** Save process configurations for automatic startup
- **Process I/O Management:** Send input to running processes
- **Pagination:** Efficient log viewing with pagination support

### Advanced Features
- **Thread-safe Operations:** Concurrent process management with proper locking
- **Single Instance Protection:** Prevents multiple supervisor instances
- **Graceful Shutdown:** Clean process termination and resource cleanup
- **Zero Dependencies Runtime:** Self-contained binary after compilation

## Use Cases & Applications

### DevOps & System Administration
- **Server Process Management:** Monitor web servers, databases, and microservices
- **Deployment Automation:** Manage application deployments and rollbacks
- **Health Monitoring:** Track critical system processes and restart on failure
- **Log Aggregation:** Centralized logging for distributed applications

### Development & Testing
- **Development Environment:** Manage multiple development servers (frontend, backend, databases)
- **Integration Testing:** Control test environments and dependencies
- **Build Automation:** Manage build processes and CI/CD pipelines
- **Mock Services:** Control mock APIs and testing infrastructure

### Data Processing & Analytics
- **ETL Pipelines:** Manage data extraction, transformation, and loading processes
- **Batch Processing:** Schedule and monitor periodic data processing jobs
- **ML Model Training:** Supervise long-running machine learning training processes
- **Data Synchronization:** Manage data sync processes between systems

### Game Development & Media
- **Game Servers:** Manage multiplayer game server instances
- **Media Processing:** Control video/audio encoding and streaming processes
- **Content Delivery:** Manage CDN edge processes and cache warming

### Enterprise Applications
- **Microservices Management:** Orchestrate service dependencies in development
- **Legacy System Integration:** Bridge old systems with modern APIs
- **Compliance Monitoring:** Track processes for audit and compliance requirements

## Installation

### Prerequisites
- Rust toolchain 1.70+ (https://rustup.rs/)
- Git

### From Source
```bash
# Clone the repository
git clone https://github.com/philopaterwaheed/exeio.git
cd exeio

# Build the release version
cargo build --release

# Install system-wide (optional)
sudo cp target/release/exeio /usr/local/bin/

# Or run directly
./target/release/exeio --help
```

### Binary Installation
```bash
# Download pre-built binary (when available)
wget https://github.com/philopaterwaheed/exeio/releases/latest/download/exeio-linux-x64
chmod +x exeio-linux-x64
sudo mv exeio-linux-x64 /usr/local/bin/exeio
```

## Quick Start

### 1. Start the Supervisor
```bash
# Start with default settings (localhost:8080)
exeio

# Custom host and port
exeio --host 0.0.0.0 --port 3000

# With custom API key
exeio --api-key "my-secure-key-123"
```

**Example Output:**
```
Process Supervisor starting on port 8080 at 127.0.0.1
API Key: exeio_philo1a2b3c4d5e6f
NOTE: All endpoints except /info require the 'exeio-api-key' header with the above key
Logs directory: /home/user/.local/share/exeio/logs
Config file: /home/user/.config/exeio/processes.json
```

### 2. Add Your First Process
```bash
# Add a simple web server
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: exeio_philo1a2b3c4d5e6f" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "web-server",
    "command": "python3",
    "args": ["-m", "http.server", "8000"],
    "working_dir": "/var/www/html",
    "auto_restart": true,
    "save_for_next_run": true
  }'
```

### 3. Monitor Process
```bash
# List all processes
curl -H "exeio-api-key: exeio_philo1a2b3c4d5e6f" \
  http://localhost:8080/list

# View process logs
curl -H "exeio-api-key: exeio_philo1a2b3c4d5e6f" \
  "http://localhost:8080/logs/web-server?page=1&page_size=50"
```

## Configuration

### Command Line Options
```bash
exeio --help
```

**Options:**
- `-H, --host <HOST>`: Host to bind to (default: 127.0.0.1)
- `-P, --port <PORT>`: Port to bind to (default: 8080)
- `-k, --api-key <KEY>`: Custom API key for authentication

### Process Configuration
```json
{
  "id": "unique-process-id",
  "command": "executable-name",
  "args": ["arg1", "arg2"],
  "working_dir": "/optional/working/directory",
  "auto_restart": true,
  "save_for_next_run": true,
  "periodic": false,
  "period_seconds": 60
}
```

## Auto-Restart Feature

The auto-restart feature is one of exeio's most powerful capabilities, providing automatic process recovery to ensure high availability and reliability of your applications.

### What is Auto-Restart?

Auto-restart monitors running processes and automatically restarts them when they exit unexpectedly. This feature helps maintain service uptime by recovering from:

- **Application crashes** (exit code ≠ 0)
- **Normal exits** (exit code = 0) that shouldn't terminate the service
- **Memory leaks** causing process termination
- **External kills** (signals from other processes)
- **System resource exhaustion**

### When to Use Auto-Restart

#### Ideal Use Cases

- **Web Servers & APIs:** Keep HTTP services running continuously
- **Database Processes:** Maintain database availability
- **Background Workers:** Ensure queue processors stay active
- **Microservices:** Maintain service mesh components
- **Development Servers:** Automatically restart dev environments
- **Data Processing:** Restart failed ETL jobs
- **Real-time Applications:** Keep streaming/chat services alive
- **System Daemons:** Maintain critical system processes

#### When NOT to Use Auto-Restart

- **One-time Scripts:** Tasks meant to run once and exit
- **Batch Jobs:** Scheduled tasks with defined completion
- **Manual Processes:** Interactive applications requiring user input
- **Testing Scripts:** Temporary validation or testing processes
- **Migration Scripts:** Database migrations or one-time setup tasks

### How Auto-Restart Works

1. **Process Monitoring:** exeio starts a dedicated monitor thread for each auto-restart enabled process
2. **Exit Detection:** The monitor detects when the process terminates (any exit code)
3. **Restart Decision:** Determines if restart should occur based on process status
4. **Intelligent Delays:** Implements exponential backoff to prevent rapid restart loops
5. **Status Tracking:** Updates process status and logs all restart activity

### Restart Delay Algorithm

exeio uses intelligent exponential backoff to prevent restart loops and system overload:

```
Base Delays:
• Restarts 1-3:   2 seconds
• Restarts 4-6:   5 seconds  
• Restarts 7-10:  15 seconds
• Restarts 11-15: 30 seconds
• Restarts 16+:   60 seconds

Rapid Restart Penalty:
• If process exits within 10 seconds: +20 seconds delay
```

**Example Timeline:**
```
Restart #1: 2s delay
Restart #2: 2s delay  
Restart #3: 2s delay (if exits quickly: 22s delay)
Restart #4: 5s delay
Restart #7: 15s delay
Restart #16: 60s delay
```

### Configuration

Enable auto-restart when adding a process:

```json
{
  "id": "web-server",
  "command": "node",
  "args": ["server.js"],
  "auto_restart": true,      // Enable auto-restart
  "save_for_next_run": true  // Optional: persist for next exeio startup
}
```

### Manual Stop vs Auto-Restart

Auto-restart intelligently distinguishes between manual stops and unexpected exits:

- **Manual Stop:** `POST /stop/{process_id}` - Process will NOT auto-restart
- **Unexpected Exit:** Process crashes or exits - Process WILL auto-restart
- **Manual Restart:** `POST /restart/{process_id}` - Restarts immediately

### Testing Auto-Restart

Here's how to test the auto-restart functionality:

#### Test 1: Normal Process Exit
```bash
# Create a test script that exits normally
echo '#!/bin/bash
echo "Process started"
sleep 5
echo "Process exiting normally"
exit 0' > test_normal_exit.sh

chmod +x test_normal_exit.sh

# Add with auto-restart
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: your-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "test-normal",
    "command": "./test_normal_exit.sh",
    "auto_restart": true
  }'

# Watch it restart automatically every ~7 seconds
curl -H "exeio-api-key: your-api-key" \
  "http://localhost:8080/logs/test-normal"
```

#### Test 2: Process Crash
```bash
# Create a script that crashes
echo '#!/bin/bash
echo "Process started"
sleep 3
echo "Simulating crash!"
exit 1' > test_crash.sh

chmod +x test_crash.sh

# Add with auto-restart
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: your-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "test-crash",
    "command": "./test_crash.sh", 
    "auto_restart": true
  }'

# Watch auto-restart after crash
curl -H "exeio-api-key: your-api-key" \
  "http://localhost:8080/logs/test-crash"
```

#### Test 3: Manual Stop (No Restart)
```bash
# Stop the process manually
curl -X POST http://localhost:8080/stop/test-crash \
  -H "exeio-api-key: your-api-key"

# Verify it stays stopped
curl -H "exeio-api-key: your-api-key" \
  http://localhost:8080/list | jq '.[] | select(.id == "test-crash")'
```

### Monitoring Auto-Restart Activity

#### Log Messages to Watch For

```bash
# Auto-restart monitoring started
[2025-07-16 04:10:02] SYSTEM: Auto-restart monitor started for process 'web-server' (PID: 12345)

# Process exit detected  
[2025-07-16 04:10:07] SYSTEM: Auto-restart monitor detected process 'web-server' (PID: 12345) has exited with status: exit status: 1

# Restart decision
[2025-07-16 04:10:07] SYSTEM: Auto-restart decision for 'web-server': should_restart=true, was_manual_stop=false, delay=2s

# Restart initiated
[2025-07-16 04:10:07] SYSTEM: Initiating auto-restart for process 'web-server' (PID: 12345) in 2s

# Different restart reasons
[2025-07-16 04:10:09] SYSTEM: Auto-restarting process 'web-server' after crash with exit code 1 (PID: 12345)
[2025-07-16 04:10:09] SYSTEM: Auto-restarting process 'web-server' after normal exit (PID: 12345) 
[2025-07-16 04:10:09] SYSTEM: Auto-restarting process 'web-server' after external kill signal 9 (PID: 12345)
```

#### Status Monitoring
```bash
# Check process status
curl -H "exeio-api-key: your-api-key" http://localhost:8080/list

# Key fields for auto-restart monitoring:
{
  "id": "web-server",
  "status": "running",          // Current status
  "auto_restart": true,         // Auto-restart enabled
  "run_count": 5,              // Number of times restarted  
  "last_run": "2025-07-16T04:10:09Z"  // Last restart time
}
```

### Best Practices

#### 1. Resource Management
```bash
# Ensure your processes handle signals gracefully
trap 'echo "Received SIGTERM, cleaning up..."; exit 0' SIGTERM
```

#### 2. Health Check Integration
```bash
# Include health checks in your applications
while true; do
  if ! health_check; then
    echo "Health check failed, exiting for restart"
    exit 1
  fi
  sleep 30
done
```

#### 3. Log Rotation
```bash
# Monitor log file sizes for auto-restart processes
ls -lh ~/.local/share/exeio/logs/
```

#### 4. Production Deployment
```json
{
  "id": "production-api",
  "command": "node",
  "args": ["server.js"],
  "working_dir": "/app",
  "auto_restart": true,
  "save_for_next_run": true  // Persist across exeio restarts
}
```

### Common Issues & Solutions

#### Issue: Restart Loop
**Symptoms:** Process keeps restarting rapidly
**Solution:** Check logs for startup errors, fix application issues

#### Issue: Process Won't Start
**Symptoms:** Auto-restart enabled but process shows "failed" status
**Solution:** Verify command path, permissions, and working directory

#### Issue: Too Many Restarts
**Symptoms:** High restart counts
**Solution:** Investigate application stability, add health checks

### Performance Impact

Auto-restart monitoring has minimal overhead:
- **Memory:** ~1-2MB per monitored process
- **CPU:** Negligible (event-driven monitoring)
- **Disk:** Only log file writes

## API Reference

### Authentication
All endpoints except `/info` require the `exeio-api-key` header:
```
exeio-api-key: your-api-key-here
```

### Endpoints

#### Process Management

**Add Process**
```http
POST /add
Content-Type: application/json

{
  "id": "my-process",
  "command": "node",
  "args": ["server.js"],
  "working_dir": "/app",
  "auto_restart": true,
  "save_for_next_run": true,
  "periodic": false,
  "period_seconds": null
}
```

**Restart Process**
```http
POST /restart/{process_id}
```

**Stop Process**
```http
POST /stop/{process_id}
```

**Remove Process**
```http
POST /remove/{process_id}
```

**List Processes**
```http
GET /list
```

**Process Logs**
```http
GET /logs/{process_id}?page=1&page_size=50
```

#### Bulk Operations

**Restart All Processes**
```http
POST /restart-all
```

**Stop All Processes**
```http
POST /stop-all
```

#### Process Interaction

**Send Input to Process**
```http
POST /input/{process_id}
Content-Type: application/json

{
  "input": "command to send to process stdin"
}
```

**Clear Process Log**
```http
POST /clear-log/{process_id}
```

#### System Information

**Supervisor Info** (No auth required)
```http
GET /info
```

**Shutdown Supervisor**
```http
POST /shutdown
```

## Examples

### Example 1: Web Development Environment
```bash
# Start multiple development servers
API_KEY="your-api-key"

# Backend API server
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "api-server",
    "command": "npm",
    "args": ["run", "dev"],
    "working_dir": "/projects/api",
    "auto_restart": true,
    "save_for_next_run": true
  }'

# Frontend development server
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "frontend",
    "command": "npm",
    "args": ["run", "serve"],
    "working_dir": "/projects/frontend",
    "auto_restart": true,
    "save_for_next_run": true
  }'

# Database
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "database",
    "command": "docker",
    "args": ["run", "--rm", "-p", "5432:5432", "-e", "POSTGRES_PASSWORD=dev", "postgres:13"],
    "auto_restart": true,
    "save_for_next_run": false
  }'
```

### Example 2: Periodic Data Processing
```bash
# Schedule a data backup every hour
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "hourly-backup",
    "command": "/scripts/backup.sh",
    "args": ["--incremental"],
    "working_dir": "/var/backups",
    "periodic": true,
    "period_seconds": 3600,
    "save_for_next_run": true
  }'

# Log processing every 5 minutes
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "log-processor",
    "command": "python3",
    "args": ["process_logs.py"],
    "working_dir": "/opt/log-processor",
    "periodic": true,
    "period_seconds": 300,
    "save_for_next_run": true
  }'
```

### Example 3: Microservices Management
```bash
# Service discovery
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "consul",
    "command": "consul",
    "args": ["agent", "-dev"],
    "auto_restart": true,
    "save_for_next_run": true
  }'

# User service
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "user-service",
    "command": "java",
    "args": ["-jar", "user-service.jar"],
    "working_dir": "/services/user",
    "auto_restart": true,
    "save_for_next_run": true
  }'

# Order service
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "order-service",
    "command": "node",
    "args": ["index.js"],
    "working_dir": "/services/order",
    "auto_restart": true,
    "save_for_next_run": true
  }'
```

### Example 4: Production Auto-Restart Setup
```bash
# Critical web service with auto-restart
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "production-api",
    "command": "node",
    "args": ["dist/server.js"],
    "working_dir": "/app",
    "auto_restart": true,
    "save_for_next_run": true
  }'

# Background job processor that should always run
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "job-processor",
    "command": "python3",
    "args": ["worker.py"],
    "working_dir": "/app/workers",
    "auto_restart": true,
    "save_for_next_run": true
  }'

# Monitor the auto-restart activity
curl -H "exeio-api-key: $API_KEY" \
  "http://localhost:8080/logs/production-api?page=1&page_size=20"

# Check restart counts and status
curl -H "exeio-api-key: $API_KEY" \
  http://localhost:8080/list | jq '.[] | {id, status, run_count, auto_restart}'
```

### Example 5: Mixed Auto-Restart and Manual Processes  
```bash
# Long-running service that should auto-restart
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "redis-server",
    "command": "redis-server",
    "args": ["--port", "6379"],
    "auto_restart": true,
    "save_for_next_run": true
  }'

# One-time migration script (no auto-restart)
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "database-migration",
    "command": "npm",
    "args": ["run", "migrate"],
    "working_dir": "/app",
    "auto_restart": false,
    "save_for_next_run": false
  }'

# Development server (auto-restart for crashes only)
curl -X POST http://localhost:8080/add \
  -H "exeio-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "dev-server",
    "command": "npm",
    "args": ["run", "dev"],
    "working_dir": "/app",
    "auto_restart": true,
    "save_for_next_run": false
  }'
```

## Architecture

### System Design
```text
┌─────────────────┐    HTTP/REST     ┌─────────────────┐
│   Client Apps   │ ◄──────────────► │   exeio API     │
│                 │                  │   (Auth Layer)  │
└─────────────────┘                  └─────────────────┘
                                              │
                                              ▼
                                     ┌─────────────────┐
                                     │ Process Manager │
                                     │ (Thread-safe)   │
                                     └─────────────────┘
                                              │
                        ┌─────────────────────┼─────────────────────┐
                        ▼                     ▼                     ▼
                ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
                │  Process A   │    │  Process B   │    │  Process C   │
                │  (Regular)   │    │ (Periodic)   │    │(Auto-restart)│
                └──────────────┘    └──────────────┘    └──────────────┘
                        │                     │                     │
                        ▼                     ▼                     ▼
                ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
                │  Log File A  │    │  Log File B  │    │  Log File C  │
                └──────────────┘    └──────────────┘    └──────────────┘
```

### Key Components
- **REST API Server:** Warp-based HTTP server with authentication
- **Process Manager:** Thread-safe process lifecycle management
- **Logging System:** Per-process and system logging with file locking
- **Configuration Manager:** Persistent process configuration storage
- **Security Layer:** API key authentication and single-instance protection

## Security

### Authentication
- **API Key Protection:** All management endpoints require valid API key
- **Secure Key Generation:** Cryptographically secure default key generation
- **Header-based Auth:** API key transmitted via HTTP headers

### Process Isolation
- **Working Directory Control:** Each process runs in specified directory
- **Environment Separation:** Isolated process environments
- **Resource Management:** Proper cleanup on process termination

### Best Practices
```bash
# Use strong API keys in production
exeio --api-key "$(openssl rand -hex 32)"

# Bind to localhost only for local development
exeio --host 127.0.0.1

# Use reverse proxy for production
# nginx/apache → exeio (with proper SSL termination)
```

## Logging & Monitoring

### Log Files Structure
```
~/.local/share/exeio/logs/
├── exeio.log              # Supervisor system log
├── process-a.log          # Individual process logs
├── process-b.log
└── ...
```

### Log Format
```
[2025-07-15 10:30:45] SYSTEM 127.0.0.1:8080: exeio process supervisor started
[2025-07-15 10:31:02] STDOUT: Server listening on port 3000
[2025-07-15 10:31:15] STDERR: Warning: deprecated API usage
[2025-07-15 10:35:20] SYSTEM 127.0.0.1:8080: Process stopped manually
```

### Monitoring Integration
```bash
# Monitor with tail
tail -f ~/.local/share/exeio/logs/exeio.log

# Integration with systemd journal
journalctl -f -u exeio

# Log rotation setup
logrotate /etc/logrotate.d/exeio
```

## System Service Setup

### systemd Service
Create `/etc/systemd/system/exeio.service`:
```ini
[Unit]
Description=exeio Process Supervisor
After=network.target

[Service]
Type=simple
User=exeio
Group=exeio
ExecStart=/usr/local/bin/exeio --host 0.0.0.0 --port 8080 --api-key your-secure-key
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start service
sudo systemctl enable exeio
sudo systemctl start exeio
sudo systemctl status exeio
```

## Performance & Scalability

### Resource Usage
- **Memory:** ~10-50MB base usage (depends on number of processes)
- **CPU:** Minimal overhead, event-driven architecture
- **Disk:** Log files (configurable rotation recommended)

### Limits
- **Concurrent Processes:** Limited by system resources
- **API Throughput:** ~1000+ requests/second (varies by hardware)
- **Log File Size:** Unlimited (implement rotation for production)

## Troubleshooting

### Common Issues

**1. Permission Denied**
```bash
# Ensure proper permissions for log directory
chmod 755 ~/.local/share/exeio/logs/
```

**2. Port Already in Use**
```bash
# Check what's using the port
netstat -tlnp | grep :8080
# Use different port
exeio --port 8081
```

**3. Process Won't Start**
```bash
# Check process logs
curl -H "exeio-api-key: your-key" \
  "http://localhost:8080/logs/process-id"
```

**4. API Key Issues**
```bash
# Verify API key in headers
curl -v -H "exeio-api-key: wrong-key" \
  http://localhost:8080/list
```

## Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

### Development Setup
```bash
git clone https://github.com/philopaterwaheed/exeio.git
cd exeio
cargo test
cargo run -- --help
```

### Testing
```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test integration

# Run with coverage
cargo tarpaulin --out Html
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

- **Issues:** [GitHub Issues](https://github.com/philopaterwaheed/exeio/issues)
- **Discussions:** [GitHub Discussions](https://github.com/philopaterwaheed/exeio/discussions)
- **Documentation:** [Wiki](https://github.com/philopaterwaheed/exeio/wiki)

---

> **Author:** [philopaterwaheed](https://github.com/philopaterwaheed)  
> **Version:** 1.0.0  
> **Made with ❤️ in Rust**
