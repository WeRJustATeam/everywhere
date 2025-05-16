use std::io::{self};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tele_framework::{
    LogicalModule, WSResult, define_module, define_framework
};
use tele_p2p::{
    config::NodesConfig,
    m_p2p::{P2pModule, P2pModuleView, P2pModuleViewTrait, P2pModuleNewArg, NodeID, P2pModuleAccessTrait},
    result::P2PResult,
    msg_pack::{MsgPack,  MsgHandler},
};
use tracing::{info, error};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};
use serde::{Serialize, Deserialize};
use std::time::Duration;
use async_trait::async_trait;
use lazy_static::lazy_static;
use paste::paste;


// 共享状态：用于存储最新消息
lazy_static! {
    static ref LATEST_MESSAGE: Mutex<Option<String>> = Mutex::new(None);
}

// 聊天消息结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ChatMessage {
    sender_id: NodeID,
    sender_name: String,
    content: String,
    timestamp: u64,
}

// 手动实现msg_id，避免与默认实现冲突
impl ChatMessage {
    fn get_msg_id() -> u32 {
        // 使用固定ID
        1001
    }
}

// 聊天模块
pub struct ChatModule {
    view: ChatModuleView,
    node_id: NodeID,
    node_name: String,
    target_id: NodeID,
}

// 聊天模块参数
#[derive(Debug, Clone)]
pub struct ChatModuleNewArg {
    node_name: String,
}

impl ChatModuleNewArg {
    pub fn new(node_name: String) -> Self {
        Self { node_name }
    }
}

#[async_trait]
impl LogicalModule for ChatModule {
    type View = ChatModuleView;
    type NewArg = ChatModuleNewArg;

    fn name(&self) -> &str {
        "ChatModule"
    }

    async fn init(view: Self::View, arg: Self::NewArg) -> WSResult<Self> {
        info!("初始化 ChatModule");
        
        // 获取本节点信息
        let p2p = view.p2p_module();
        let node_id = p2p.nodes_config.this_node();
        let target_id = if node_id == 1 { 2 } else { 1 };
        info!("聊天节点 {} ({}) 初始化中", arg.node_name, node_id);
        
        // 注册消息处理器
        let msg_handler = MsgHandler::<ChatMessage>::new();
        let this_node_copy = node_id; // 复制以便在闭包中使用
        
        msg_handler.regist(p2p, move |_resp, msg: ChatMessage| {
            // 只显示来自其他节点的消息
            if msg.sender_id != this_node_copy {
                println!("\n[{}] {}: {}", 
                         msg.sender_name, 
                         msg.timestamp, 
                         msg.content);
                println!("请输入回复: ");
                
                // 保存消息
                let mut latest = LATEST_MESSAGE.lock().unwrap();
                *latest = Some(format!("[{}] {}", msg.sender_name, msg.content));
            }
            Ok(())
        });
        
        // 创建聊天模块
        let module = Self {
            view: view.clone(),
            node_id,
            node_name: arg.node_name.clone(),
            target_id,
        };
        
        // 启动用户输入处理任务
        let view_clone = view.clone();
        let node_name_clone = arg.node_name.clone();
        let node_id_copy = node_id;
        let target_id_copy = target_id;
        
        // 打印欢迎信息
        let target_name = if node_id == 1 { "Bob" } else { "Alice" };
        println!("\n聊天程序已启动!");
        println!("你是: {} (节点{})", arg.node_name, node_id);
        println!("你正在与 {} (节点{}) 聊天", target_name, target_id);
        println!("输入消息并按Enter发送，或输入'exit'退出");
        println!("请输入消息: ");
        
        // 使用spawn_blocking处理阻塞的stdin读取
        tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let mut buffer = String::new();
            
            loop {
                buffer.clear();
                if stdin.read_line(&mut buffer).is_err() {
                    error!("读取输入失败");
                    break;
                }
                
                let input = buffer.trim().to_string();
                if input.eq_ignore_ascii_case("exit") {
                    break;
                }
                
                if !input.is_empty() {
                    println!("输入不为空, 发送 {}", input);
                    // 发送消息
                    let view_clone = view_clone.clone();
                    let content = input.clone();
                    let name = node_name_clone.clone();
                    
                    // 使用tokio::spawn启动异步任务发送消息
                    tokio::spawn(async move {
                        println!("send_chat");
                        match send_chat(&view_clone, content, node_id_copy, &name, target_id_copy).await {
                            Ok(_) => {},
                            Err(e) => error!("发送消息失败: {}", e),
                        }
                    });
                }else{
                    println!("输入为空");
                }
                
                println!("请输入消息: ");
            }
        });
        
        info!("ChatModule初始化完成");
        Ok(module)
    }

    async fn shutdown(&self) -> WSResult<()> {
        info!("关闭 ChatModule");
        Ok(())
    }
}

// 发送聊天消息
async fn send_chat(
    view: &ChatModuleView,
    content: String,
    node_id: NodeID,
    node_name: &str,
    target_id: NodeID,
) -> P2PResult<()> {    
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let msg = ChatMessage {
        sender_id: node_id,
        sender_name: node_name.to_string(),
        content,
        timestamp,
    };
    
    let p2p = view.p2p_module();
    p2p.send(target_id, ChatMessage::get_msg_id(), msg.encode())?;
    Ok(())
}

// 使用宏定义模块和框架
define_module!(ChatModule, (p2p, P2pModule));

define_framework! {
    p2p: P2pModule,
    chat: ChatModule
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
    
    info!("启动聊天程序");
    
    // 读取命令行参数
    let args: Vec<String> = std::env::args().collect();
    let node_id = if args.len() > 1 {
        match args[1].parse::<u32>() {
            Ok(id) if id == 1 || id == 2 => id,
            _ => {
                println!("无效的节点ID。必须为1或2。");
                println!("用法: cargo run --example chat [1|2]");
                return Ok(());
            }
        }
    } else {
        println!("请指定节点ID (1 或 2)");
        println!("用法: cargo run --example chat [1|2]");
        return Ok(());
    };
    
    // 设置节点名称
    let node_name = if node_id == 1 { "Alice" } else { "Bob" };
    info!("启动节点 {} ({})", node_name, node_id);
    
    // 创建节点配置
    let config = NodesConfig::new(
        node_id,
        vec![
            (1, "127.0.0.1:10001".parse().unwrap()),
            (2, "127.0.0.1:10002".parse().unwrap()),
        ],
    );
    
    // 创建模块参数
    let p2p_arg = P2pModuleNewArg::new(config);
    let chat_arg = ChatModuleNewArg::new(node_name.to_string());
    
    // 创建框架参数
    let framework_args = FrameworkArgs {
        p2p_arg,
        chat_arg,
    };
    
    // 创建并初始化框架
    let framework = Framework::new();
    info!("初始化框架");
    framework.init(framework_args).await?;
    
    // 等待连接建立
    info!("等待5秒钟让节点互联...");
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    // 只等待Ctrl+C信号
    tokio::signal::ctrl_c().await.expect("无法监听Ctrl+C信号");
    info!("接收到Ctrl+C信号，程序退出");
    
    println!("聊天程序已关闭");
    Ok(())
} 