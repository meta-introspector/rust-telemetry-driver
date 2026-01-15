use std::env;
use std::process::{Command, Stdio};
use std::fs::OpenOptions;
use std::io::{Write, Read, BufReader, BufRead};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use std::thread;
use std::sync::mpsc;
use uuid::Uuid;

#[derive(serde::Serialize)]
struct TelemetryEvent {
    event_id: String,
    event_type: String,
    timestamp: f64,
    pid: i32,
    ppid: i32,
    session_id: String,
    command: Vec<String>,
    cwd: String,
    env: HashMap<String, String>,
    resource_usage: Option<ResourceUsage>,
    duration_ms: Option<u128>,
    exit_code: Option<i32>,
    stdout_lines: Option<Vec<String>>,
    stderr_lines: Option<Vec<String>>,
    stdin_provided: Option<String>,
    stdout_size_bytes: Option<usize>,
    stderr_size_bytes: Option<usize>,
}

#[derive(serde::Serialize)]
struct ProcessStats {
    start_time: f64,
    end_time: f64,
    duration_ms: u128,
    exit_code: i32,
    signal: Option<i32>,
    stdout_lines: usize,
    stderr_lines: usize,
    total_output_bytes: usize,
}

#[derive(serde::Serialize)]
struct ResourceUsage {
    user_time_ms: i64,
    system_time_ms: i64,
    max_rss_kb: i64,
    page_faults: i64,
    context_switches: i64,
}

fn capture_stream_lines(mut reader: impl BufRead + Send + 'static) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut total_bytes = 0;
    
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(n) => {
                total_bytes += n;
                // Remove trailing newline for cleaner storage
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                lines.push(line);
            }
            Err(_) => break,
        }
    }
    
    (lines, total_bytes)
}

fn log_telemetry_event(event: &TelemetryEvent, telemetry_file: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(telemetry_file) {
        if let Ok(json_str) = serde_json::to_string(event) {
            let _ = writeln!(file, "{}", json_str);
        }
    }
}

fn get_resource_usage() -> Option<ResourceUsage> {
    // Simplified - just return None for now
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("🔍 Rust Telemetry Driver v0.2.0 - Comprehensive Process Capture");
        eprintln!("Usage: {} <command> [args...]", args[0]);
        eprintln!("Captures: args, return codes, stdin/stdout/stderr streams");
        std::process::exit(1);
    }
    
    let session_id = env::var("TELEMETRY_SESSION_ID")
        .unwrap_or_else(|_| Uuid::new_v4().to_string());
    
    let telemetry_file = env::var("TELEMETRY_LOG")
        .unwrap_or_else(|_| format!("/tmp/rust_telemetry_{}.jsonl", 
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()));
    
    let start_time = SystemTime::now();
    let start_timestamp = start_time.duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    
    // Capture pre-execution state
    let pre_event = TelemetryEvent {
        event_id: Uuid::new_v4().to_string(),
        event_type: "process_start".to_string(),
        timestamp: start_timestamp,
        pid: std::process::id() as i32,
        ppid: 0, // Simplified
        session_id: session_id.clone(),
        command: args[1..].to_vec(),
        cwd: env::current_dir().unwrap().to_string_lossy().to_string(),
        env: env::vars().collect(),
        resource_usage: get_resource_usage(),
        duration_ms: None,
        exit_code: None,
        stdout_lines: None,
        stderr_lines: None,
        stdin_provided: None,
        stdout_size_bytes: None,
        stderr_size_bytes: None,
    };
    
    log_telemetry_event(&pre_event, &telemetry_file);
    
    eprintln!("🚀 [{}] Executing: {}", 
        session_id, 
        args[1..].join(" "));
    
    // Execute command with full stdio capture
    let mut cmd = Command::new(&args[1]);
    cmd.args(&args[2..]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::piped());
    
    let mut child = cmd.spawn().expect("Failed to spawn command");
    
    // Get handles for stdout and stderr
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let stderr = child.stderr.take().expect("Failed to get stderr");
    
    // Spawn threads to capture stdout and stderr
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    
    // Capture stdout
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let (lines, bytes) = capture_stream_lines(reader);
        stdout_tx.send((lines, bytes)).unwrap();
    });
    
    // Capture stderr  
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let (lines, bytes) = capture_stream_lines(reader);
        stderr_tx.send((lines, bytes)).unwrap();
    });
    
    // Wait for process to complete
    let output = child.wait().expect("Failed to wait for child");
    let end_time = SystemTime::now();
    let duration = end_time.duration_since(start_time).unwrap();
    
    // Collect stdout and stderr results
    let (stdout_lines, stdout_bytes) = stdout_rx.recv().unwrap();
    let (stderr_lines, stderr_bytes) = stderr_rx.recv().unwrap();
    
    // Print captured output to maintain normal behavior
    for line in &stdout_lines {
        println!("{}", line);
    }
    for line in &stderr_lines {
        eprintln!("{}", line);
    }
    
    // Create process statistics
    let process_stats = ProcessStats {
        start_time: start_timestamp,
        end_time: end_time.duration_since(UNIX_EPOCH).unwrap().as_secs_f64(),
        duration_ms: duration.as_millis(),
        exit_code: output.code().unwrap_or(-1),
        signal: None, // Could be enhanced to capture signals
        stdout_lines: stdout_lines.len(),
        stderr_lines: stderr_lines.len(),
        total_output_bytes: stdout_bytes + stderr_bytes,
    };
    
    // Capture post-execution state
    let post_event = TelemetryEvent {
        event_id: Uuid::new_v4().to_string(),
        event_type: "process_end".to_string(),
        timestamp: process_stats.end_time,
        pid: std::process::id() as i32,
        ppid: 0,
        session_id: session_id.clone(),
        command: args[1..].to_vec(),
        cwd: env::current_dir().unwrap().to_string_lossy().to_string(),
        env: HashMap::new(), // Don't duplicate env in end event
        resource_usage: get_resource_usage(),
        duration_ms: Some(process_stats.duration_ms),
        exit_code: Some(process_stats.exit_code),
        stdout_lines: Some(stdout_lines),
        stderr_lines: Some(stderr_lines),
        stdin_provided: None, // Could be enhanced to capture stdin
        stdout_size_bytes: Some(stdout_bytes),
        stderr_size_bytes: Some(stderr_bytes),
    };
    
    log_telemetry_event(&post_event, &telemetry_file);
    
    // Log process statistics summary
    let stats_event = TelemetryEvent {
        event_id: Uuid::new_v4().to_string(),
        event_type: "process_stats".to_string(),
        timestamp: process_stats.end_time,
        pid: std::process::id() as i32,
        ppid: 0,
        session_id,
        command: args[1..].to_vec(),
        cwd: env::current_dir().unwrap().to_string_lossy().to_string(),
        env: HashMap::from([
            ("duration_ms".to_string(), process_stats.duration_ms.to_string()),
            ("stdout_lines".to_string(), process_stats.stdout_lines.to_string()),
            ("stderr_lines".to_string(), process_stats.stderr_lines.to_string()),
            ("total_bytes".to_string(), process_stats.total_output_bytes.to_string()),
        ]),
        resource_usage: None,
        duration_ms: Some(process_stats.duration_ms),
        exit_code: Some(process_stats.exit_code),
        stdout_lines: None,
        stderr_lines: None,
        stdin_provided: None,
        stdout_size_bytes: Some(stdout_bytes),
        stderr_size_bytes: Some(stderr_bytes),
    };
    
    log_telemetry_event(&stats_event, &telemetry_file);
    
    eprintln!("✅ [{}] Completed in {:.2}ms | Exit: {} | Out: {} lines/{} bytes | Err: {} lines/{} bytes", 
        pre_event.session_id,
        process_stats.duration_ms,
        process_stats.exit_code,
        process_stats.stdout_lines,
        stdout_bytes,
        process_stats.stderr_lines,
        stderr_bytes);
    eprintln!("📊 Telemetry: {}", telemetry_file);
    
    std::process::exit(process_stats.exit_code);
}
