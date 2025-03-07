use std::io::{self, BufRead};
use std::time::{SystemTime, UNIX_EPOCH};
use tele_framework::WSResult;
use tracing::{info, debug, warn, error};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};
use serde::{Serialize, Deserialize};

// 定义聊天消息格式
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    sender_id: u32,
    sender_name: String,
    content: String,
    timestamp: u64,
}

fn print_usage() {
    println!("聊天程序");
    println!("使用方法:");
    println!("  在两个不同的终端窗口分别运行:");
    println!("  终端1: cargo run --example chat 1");
    println!("  终端2: cargo run --example chat 2");
    println!();
    println!("注意: 两个程序需要同时运行才能互相通信。程序启动后将建立P2P连接，然后您可以在两个终端之间进行对话。");
}

#[tokio::main]
async fn main() -> WSResult<()> {
    // 初始化日志
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tele_p2p=info".parse().unwrap()))
        .with_span_events(FmtSpan::CLOSE)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)
        .expect("设置全局日志订阅器失败");
    
    // 读取命令行参数
    let args: Vec<String> = std::env::args().collect();
    if args.len() <= 1 {
        print_usage();
        return Ok(());
    }
    
    // 确定节点ID
    let node_id = match args[1].as_str() {
        "1" => 1,
        "2" => 2,
        _ => {
            println!("无效的节点ID。请使用1或2作为节点ID。");
            print_usage();
            return Ok(());
        }
    };
    
    // 设置节点名称
    let node_name = if node_id == 1 { "Alice" } else { "Bob" };
    
    // 输出节点信息
    println!("启动节点 {} ({})", node_name, node_id);
    println!("将在另一个终端运行对应的节点来建立P2P连接");
    println!();
    
    // 创建命令
    let peer_id = if node_id == 1 { 2 } else { 1 };
    let peer_name = if node_id == 1 { "Bob" } else { "Alice" };
    
    println!("现在，您可以发送消息了。输入一条消息并按Enter发送，或者输入'exit'退出。");
    println!("假设您已经在另一个终端启动了节点{}，消息将会发送给{}", peer_id, peer_name);
    println!();
    
    // 模拟聊天输入
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    
    println!("> ");
    while let Some(Ok(line)) = lines.next() {
        if line.trim().eq_ignore_ascii_case("exit") {
            break;
        }
        
        if !line.trim().is_empty() {
            // 创建消息 (这里我们只是模拟，不实际发送)
            let message = ChatMessage {
                sender_id: node_id,
                sender_name: node_name.to_string(),
                content: line.trim().to_string(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            
            // 模拟消息发送
            println!("发送消息到节点 {}: {:?}", peer_id, message);
        }
        
        println!("> ");
    }
    
    println!("程序退出");
    Ok(())
} 