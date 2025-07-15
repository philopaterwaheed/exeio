# exeio

A powerful process supervisor written in Rust that helps developers, system administrators, and DevOps engineers run, monitor, and manage processes remotely through a secure REST API interface.

## Table of Contents

- [Features](#features)
- [Use Cases & Applications](#use-cases--applications)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Examples](#examples)
- [Architecture](#architecture)
- [Security](#security)
- [Logging & Monitoring](#logging--monitoring)
- [Contributing](#contributing)
- [License](#license)

## Features

### Core Capabilities
- **process Supervision:** Start, stop, restart, and monitor multiple processes 
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

## Architecture

### System Design
```
┌─────────────────┐    HTTP/REST    ┌─────────────────┐
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
                │  (Regular)   │    │ (Periodic)   │    │  (Auto-restart) │
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
