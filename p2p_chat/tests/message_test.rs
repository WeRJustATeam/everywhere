use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};
use std::thread;
use std::path::Path;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use serde_yaml;

#[derive(Debug, Serialize, Deserialize)]
struct NodeConfig {
    bind_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    nodes: std::collections::HashMap<String, NodeConfig>,
}

// 测试配置
const TEST_CONFIG_PATH: &str = "test_config.yaml";
const NODE1_ID: &str = "test_node1";
const NODE2_ID: &str = "test_node2";
const NODE3_ID: &str = "test_node3";
const NODE1_ADDR: &str = "127.0.0.1:7001";
const NODE2_ADDR: &str = "127.0.0.1:7002";
const NODE3_ADDR: &str = "127.0.0.1:7003";
const TEST_MESSAGE: &str = "这是一条测试消息从节点1发送到节点2和节点3";

/// 测试结果结构
#[derive(Debug)]
struct TestResult {
    success: bool,
    message: Option<String>,
    errors: Vec<String>,
    node2_output: Vec<String>,
}

#[test]
fn test_message_transmission() {
    // 创建测试结果对象
    let mut result = TestResult {
        success: false,
        message: None,
        errors: Vec::new(),
        node2_output: Vec::new(),
    };

    // 消息接收标志
    let received_message = Arc::new(AtomicBool::new(false));
    
    // 捕获的输出
    let captured_output = Arc::new(Mutex::new(Vec::new()));
    
    // 错误信息
    let errors = Arc::new(Mutex::new(Vec::new()));
    
    // 连接成功标志
    let connection_established = Arc::new(AtomicBool::new(false));
    
    // 创建测试配置文件
    println!("创建测试配置文件...");
    create_test_config();
    
    // 1. 首先启动节点1（发送方）
    println!("启动节点1（发送方）...");
    let mut node1 = Command::new("cargo")
        .args(&["run", "--", "start", "-c", TEST_CONFIG_PATH, "-i", NODE1_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动节点1失败");
    
    // 等待3秒，确保节点1完全启动
    println!("等待节点1启动完成...");
    thread::sleep(Duration::from_secs(3));
    
    // 2. 启动节点3（观察者）
    println!("启动节点3（观察者）...");
    let mut node3 = Command::new("cargo")
        .args(&["run", "--", "start", "-c", TEST_CONFIG_PATH, "-i", NODE3_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动节点3失败");
    
    // 3. 启动节点2（接收方）
    println!("启动节点2（接收方）...");
    let mut node2 = Command::new("cargo")
        .args(&["run", "--", "start", "-c", TEST_CONFIG_PATH, "-i", NODE2_ID])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动节点2失败");
    
    let node2_stdout = node2.stdout.take().expect("无法获取节点2的stdout");
    let node2_stderr = node2.stderr.take().expect("无法获取节点2的stderr");
    
    let node2_reader = BufReader::new(node2_stdout);
    let node2_stderr_reader = BufReader::new(node2_stderr);
    
    let received_message_clone = received_message.clone();
    let captured_output_clone = captured_output.clone();
    let errors_clone = errors.clone();
    let connection_established_clone = connection_established.clone();
    
    // 启动stdout监听线程
    let _stdout_checker = thread::spawn(move || {
        for line in node2_reader.lines() {
            if let Ok(line) = line {
                // 存储输出
                captured_output_clone.lock().unwrap().push(line.clone());
                
                // 检查行是否包含测试消息
                if line.contains(TEST_MESSAGE) {
                    received_message_clone.store(true, Ordering::SeqCst);
                    break;
                }
                
                // 检查是否有连接成功的消息
                if line.contains("已连接到节点") || (line.contains("已将节点") && line.contains("添加到peer列表")) {
                    connection_established_clone.store(true, Ordering::SeqCst);
                }
            }
        }
    });
    
    let connection_established_clone = connection_established.clone();
    let errors_clone = errors.clone();
    
    // 启动stderr监听线程
    let _stderr_checker = thread::spawn(move || {
        for line in node2_stderr_reader.lines() {
            if let Ok(line) = line {
                // 检查是否有连接成功的消息
                if line.contains("已连接到节点") || (line.contains("已将节点") && line.contains("添加到peer列表")) {
                    connection_established_clone.store(true, Ordering::SeqCst);
                }
                
                // 只存储错误信息
                if line.contains("ERROR") {
                    errors_clone.lock().unwrap().push(line);
                }
            }
        }
    });
    
    // 等待8秒，确保节点连接已经建立
    println!("等待节点连接建立...");
    thread::sleep(Duration::from_secs(8));
    
    // 4. 从节点1发送消息
    println!("从节点1发送消息到节点2和节点3...");
    let mut node1_stdin = node1.stdin.take().expect("无法获取节点1的stdin");
    writeln!(node1_stdin, "{}", TEST_MESSAGE).expect("向节点1发送消息失败");
    
    // 5. 等待5秒，确保消息传递完成
    println!("等待消息传递...");
    thread::sleep(Duration::from_secs(5));
    
    // 6. 保存测试结果
    result.node2_output = captured_output.lock().unwrap().clone();
    result.errors = errors.lock().unwrap().clone();
    
    // 7. 判断测试结果
    result.success = received_message.load(Ordering::SeqCst);
    
    // 检查连接是否建立
    let connection_ok = connection_established.load(Ordering::SeqCst);
    
    if !connection_ok {
        println!("\n连接测试失败: 节点之间未能建立连接");
        if let Some(ref mut msg) = result.message {
            *msg = format!("{}; 并且节点之间未能建立连接", msg);
        } else {
            result.message = Some("节点之间未能建立连接".to_string());
        }
    } else {
        println!("\n连接测试成功: 节点之间成功建立了连接");
    }
    
    if result.success {
        result.message = Some(format!("节点2成功接收到节点1发送的消息: {}", TEST_MESSAGE));
        println!("\n消息测试成功: {}", result.message.as_ref().unwrap());
    } else {
        if result.message.is_none() {
            result.message = Some("节点2未能接收到节点1发送的消息".to_string());
        }
        println!("\n消息测试失败: {}", result.message.as_ref().unwrap());
        
        // 只有在失败时才打印错误信息
        if !result.errors.is_empty() {
            println!("错误信息:");
            for (i, err) in result.errors.iter().enumerate() {
                println!("  {}: {}", i+1, err);
            }
        }
    }
    
    // 将测试结果保存到日志文件
    save_test_log(&result);
    
    // 8. 终止所有节点
    node1.kill().expect("终止节点1失败");
    node2.kill().expect("终止节点2失败");
    node3.kill().expect("终止节点3失败");
    
    // 9. 断言测试结果
    assert!(result.success, "消息传输测试失败: {}", result.message.unwrap_or_else(|| "未知错误".to_string()));
}

/// 创建测试配置文件
fn create_test_config() {
    let mut config = Config {
        nodes: std::collections::HashMap::new(),
    };
    
    config.nodes.insert(NODE1_ID.to_string(), NodeConfig {
        bind_address: NODE1_ADDR.to_string(),
    });
    
    config.nodes.insert(NODE2_ID.to_string(), NodeConfig {
        bind_address: NODE2_ADDR.to_string(),
    });
    
    config.nodes.insert(NODE3_ID.to_string(), NodeConfig {
        bind_address: NODE3_ADDR.to_string(),
    });
    
    let yaml = serde_yaml::to_string(&config).expect("序列化配置失败");
    fs::write(TEST_CONFIG_PATH, yaml).expect("写入配置文件失败");
}

/// 保存测试日志
fn save_test_log(result: &TestResult) {
    let mut log = String::new();
    
    // 测试结果摘要
    log.push_str(&format!("测试结果: {}\n", if result.success { "成功" } else { "失败" }));
    if let Some(ref msg) = result.message {
        log.push_str(&format!("原因: {}\n", msg));
    }
    
    // 错误信息
    if !result.errors.is_empty() {
        log.push_str("\n错误信息:\n");
        for (i, err) in result.errors.iter().enumerate() {
            log.push_str(&format!("  {}: {}\n", i+1, err));
        }
    }
    
    // 节点2的输出
    log.push_str("\n节点2的输出:\n");
    for (i, line) in result.node2_output.iter().enumerate() {
        log.push_str(&format!("  {}: {}\n", i+1, line));
    }
    
    // 写入日志文件
    fs::write("message_test_output.log", log).expect("写入日志文件失败");
    println!("\n测试日志已保存到 message_test_output.log");
} 