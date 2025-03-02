use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use std::path::Path;
use std::fs;
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
const NODE1_ADDR: &str = "127.0.0.1:7001";
const NODE2_ADDR: &str = "127.0.0.1:7002";

// 创建测试配置文件
fn create_test_config() -> std::io::Result<()> {
    let mut nodes = std::collections::HashMap::new();
    
    nodes.insert(NODE1_ID.to_string(), NodeConfig { 
        bind_address: NODE1_ADDR.to_string() 
    });
    nodes.insert(NODE2_ID.to_string(), NodeConfig { 
        bind_address: NODE2_ADDR.to_string() 
    });
    
    let config = Config { nodes };
    let yaml = serde_yaml::to_string(&config).unwrap();
    
    fs::write(TEST_CONFIG_PATH, yaml)?;
    println!("测试配置文件已创建: {}", TEST_CONFIG_PATH);
    Ok(())
}

#[test]
fn test_basic_display() -> std::io::Result<()> {
    // 确保我们在正确的目录中
    let mut cargo_path = std::env::current_dir()?;
    while !Path::new(&cargo_path.join("Cargo.toml")).exists() {
        if !cargo_path.pop() {
            panic!("无法找到Cargo.toml，请确保在项目目录中运行测试");
        }
    }
    
    println!("在目录 {:?} 中运行测试", cargo_path);
    
    // 创建测试配置
    create_test_config()?;
    
    // 输出测试指南
    println!("\n=====================================================");
    println!("测试指南：");
    println!("1. 测试将启动两个节点窗口");
    println!("2. 在节点1窗口中输入消息并观察节点2的显示效果");
    println!("3. 验证消息是否以彩色框架格式显示，带有时间和发送者信息");
    println!("4. 测试完成后在节点窗口中输入'exit'退出");
    println!("=====================================================\n");
    
    // 启动节点1（发送方）
    println!("启动节点1 (发送方)...");
    let _node1 = Command::new("cmd")
        .current_dir(&cargo_path)
        .args(&["/C", "start", "powershell", "-Command", 
              &format!("cd {}; cargo run -- {} {}", 
                       cargo_path.display(), TEST_CONFIG_PATH, NODE1_ID)])
        .spawn()?;
    
    // 启动节点2（接收方）
    println!("启动节点2 (接收方)...");
    let _node2 = Command::new("cmd")
        .current_dir(&cargo_path)
        .args(&["/C", "start", "powershell", "-Command", 
              &format!("cd {}; cargo run -- {} {}", 
                       cargo_path.display(), TEST_CONFIG_PATH, NODE2_ID)])
        .spawn()?;
    
    // 等待用户手动测试
    println!("节点已启动，请手动测试消息发送和显示效果");
    println!("测试将在60秒后自动完成...");
    
    // 等待60秒后结束测试
    thread::sleep(Duration::from_secs(60));
    
    println!("测试完成。请在节点窗口中输入'exit'退出节点程序。");
    
    Ok(())
} 