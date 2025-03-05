use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Local;
use tracing::{debug, error, info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tele_framework::{
    LogicalModule, LogicalModuleNewArgs, LogicalModulesRef, NodeID, 
    WSResult, JoinHandleWrapper, Sys,
    logical_module_view_impl, register_module, initialize_module, define_modules,
};
use tele_p2p::{
    msg_pack::{MsgPack, RPCReq},
    TaskId, MsgId, RPCResponsor, Responser, P2PModule, P2PView,
    config::NodesConfig, config::NodeConfig,
};
use tokio::{
    io::{self, AsyncBufReadExt},
    sync::mpsc,
};
use uuid::Uuid;

// 定义ANSI颜色代码常量
const COLOR_RESET: &str = "\x1b[0m";
const COLOR_RED: &str = "\x1b[31m";
const COLOR_GREEN: &str = "\x1b[32m";
const COLOR_YELLOW: &str = "\x1b[33m";
const COLOR_BLUE: &str = "\x1b[34m";
const COLOR_MAGENTA: &str = "\x1b[35m";
const COLOR_CYAN: &str = "\x1b[36m";

// 颜色循环函数
fn get_color_for_sender(sender: &str) -> &'static str {
    // 基于发送者名称的哈希值选择颜色
    let colors = [COLOR_RED, COLOR_GREEN, COLOR_YELLOW, COLOR_BLUE, COLOR_MAGENTA, COLOR_CYAN];
    let hash = sender.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64));
    let index = (hash % colors.len() as u64) as usize;
    colors[index]
}

// 定义配置结构
#[derive(Debug, Serialize, Deserialize)]
struct Config {
    nodes: HashMap<String, ChatNodeConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatNodeConfig {
    bind_address: String,
    name: String,
}

// 聊天消息结构
#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    id: String,
    sender: String,
    content: String,
    timestamp: u64,
}

impl ChatMessage {
    fn new(sender: String, content: String) -> Self {
        let id = Uuid::new_v4().to_string();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            id,
            sender,
            content,
            timestamp,
        }
    }
    
    fn display(&self) -> String {
        let dt = chrono::DateTime::from_timestamp(self.timestamp as i64, 0).unwrap();
        let time = dt.with_timezone(&Local).format("%H:%M:%S").to_string();
        let color = get_color_for_sender(&self.sender);
        format!("[{}] {}{}{}: {}", 
            time, 
            color, 
            self.sender, 
            COLOR_RESET, 
            self.content
        )
    }
}

// 定义模块集合
define_modules! {
    (p2p, P2PModule),
    (chat, ChatNodeModule),
}

// 聊天节点模块
#[derive(Debug)]
struct ChatNodeModule {
    id: NodeID,
    name: String,
    seen_messages: Mutex<HashSet<String>>,
    messages: Mutex<Vec<ChatMessage>>,
    message_tx: mpsc::Sender<ChatMessage>,
    message_rx: Mutex<Option<mpsc::Receiver<ChatMessage>>>,
    p2p_view: P2PView,
}

// 定义聊天模块视图
logical_module_view_impl!(ChatNodeView);
logical_module_view_impl!(ChatNodeView, chat, ChatNodeModule);

// 为ChatMessage实现MsgPack特性，用于P2P传输
impl MsgPack for ChatMessage {
    fn msg_id(&self) -> MsgId {
        1 // 简单的消息ID
    }
    
    fn encode_to_vec(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    
    fn decode(bytes: bytes::Bytes) -> Result<Self, prost::DecodeError> {
        serde_json::from_slice(&bytes).map_err(|e| {
            prost::DecodeError::new(format!("Failed to decode ChatMessage: {}", e))
        })
    }
}

#[async_trait]
impl LogicalModule for ChatNodeModule {
    fn inner_new(args: LogicalModuleNewArgs) -> Self {
        // 创建消息通道
        let (tx, rx) = mpsc::channel(100);
        
        // 创建P2P视图
        let p2p_view = P2PView::new(args.logical_modules_ref.clone());
        
        Self {
            id: args.nodes_config.this.0 as u32,
            name: args.nodes_config.this.1.hostname.clone(),
            seen_messages: Mutex::new(HashSet::new()),
            messages: Mutex::new(Vec::new()),
            message_tx: tx,
            message_rx: Mutex::new(Some(rx)),
            p2p_view,
        }
    }
    
    async fn init(&self) -> WSResult<()> {
        // 注册消息处理器
        let handler = {
            let this = self.clone();
            move |resp: Responser, msg: ChatMessage| {
                let this = this.clone();
                tokio::spawn(async move {
                    if let Err(e) = this.handle_incoming_message(&msg, resp.node_id).await {
                        error!("处理消息错误: {:?}", e);
                    }
                });
                Ok(())
            }
        };
        
        // 注册处理器
        let msg_handler = tele_p2p::MsgHandler::<ChatMessage>::new();
        msg_handler.regist(self.p2p_view.p2p(), handler);
        
        info!("初始化聊天节点: {}", self.name);
        Ok(())
    }
    
    async fn start(&self) -> WSResult<Vec<JoinHandleWrapper>> {
        let mut handles = Vec::new();
        
        // 启动消息处理任务
        if let Some(rx) = self.message_rx.lock().take() {
            let p2p = self.p2p_view.p2p();
            let id = self.id;
            let name = self.name.clone();
            
            let handle = tokio::spawn(async move {
                Self::message_handling_task(p2p, id, name, rx).await;
            });
            
            handles.push(JoinHandleWrapper::new(handle));
        }
        
        info!("启动聊天节点: {}", self.name);
        Ok(handles)
    }
}

impl ChatNodeModule {
    // 处理传入消息
    async fn handle_incoming_message(&self, message: &ChatMessage, source_id: NodeID) -> WSResult<()> {
        // 检查是否已处理过此消息
        {
            let mut seen = self.seen_messages.lock();
            if seen.contains(&message.id) {
                return Ok(());
            }
            seen.insert(message.id.clone());
        }
        
        // 存储消息
        {
            let mut messages = self.messages.lock();
            messages.push(message.clone());
        }
        
        // 打印消息
        println!("{}", message.display());
        
        // 转发消息给其他节点
        for (peer_id, _) in &self.p2p_view.p2p().nodes_config.peers {
            let peer_id = *peer_id as u32;
            if peer_id != self.id && peer_id != source_id {
                let sender = tele_p2p::MsgSender::<ChatMessage>::new();
                if let Err(e) = sender.send(self.p2p_view.p2p(), peer_id, message.clone()).await {
                    warn!("转发消息到节点 {} 失败: {:?}", peer_id, e);
                }
            }
        }
        
        Ok(())
    }
    
    // 发送消息
    pub async fn send_message(&self, content: String) -> WSResult<()> {
        let message = ChatMessage::new(self.name.clone(), content);
        println!("{}", message.display());
        
        // 添加到已见消息集合
        {
            let mut seen = self.seen_messages.lock();
            seen.insert(message.id.clone());
        }
        
        // 添加到消息列表
        {
            let mut messages = self.messages.lock();
            messages.push(message.clone());
        }
        
        // 发送到消息通道
        if let Err(e) = self.message_tx.send(message).await {
            error!("发送消息到通道失败: {:?}", e);
        }
        
        Ok(())
    }
    
    // 消息处理任务
    async fn message_handling_task(
        p2p: &P2PModule,
        id: NodeID,
        name: String,
        mut rx: mpsc::Receiver<ChatMessage>,
    ) {
        info!("消息处理任务启动");
        
        while let Some(message) = rx.recv().await {
            // 广播消息到所有其他节点
            for (peer_id, _) in &p2p.nodes_config.peers {
                let peer_id = *peer_id as u32;
                if peer_id != id {
                    let sender = tele_p2p::MsgSender::<ChatMessage>::new();
                    if let Err(e) = sender.send(p2p, peer_id, message.clone()).await {
                        warn!("广播消息到节点 {} 失败: {:?}", peer_id, e);
                    }
                }
            }
        }
    }
}

impl Clone for ChatNodeModule {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            seen_messages: Mutex::new(self.seen_messages.lock().clone()),
            messages: Mutex::new(self.messages.lock().clone()),
            message_tx: self.message_tx.clone(),
            message_rx: Mutex::new(None), // 接收端不能被克隆
            p2p_view: self.p2p_view.clone(),
        }
    }
}

// 构建节点配置
fn build_nodes_config(config: &Config, node_name: &str) -> WSResult<NodesConfig> {
    let current_node = config.nodes.get(node_name).ok_or_else(|| {
        Box::new(WSError::Generic(format!("节点 {} 未在配置中找到", node_name)))
    })?;
    
    let this_addr = current_node.bind_address.parse::<SocketAddr>().map_err(|e| {
        Box::new(WSError::Generic(format!("无效的地址 {}: {}", current_node.bind_address, e)))
    })?;
    
    let mut peers = HashMap::new();
    
    for (name, node) in &config.nodes {
        if name != node_name {
            let addr = node.bind_address.parse::<SocketAddr>().map_err(|e| {
                Box::new(WSError::Generic(format!("无效的地址 {}: {}", node.bind_address, e)))
            })?;
            
            let peer_id = name.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64)) % 1000 + 1;
            peers.insert(peer_id, NodeConfig::new(
                addr,
                node.name.clone(),
                vec![],
            ));
        }
    }
    
    let this_id = node_name.bytes().fold(0u64, |acc, b| acc.wrapping_add(b as u64)) % 1000 + 1;
    let this = (
        this_id,
        NodeConfig::new(
            this_addr,
            current_node.name.clone(),
            vec![],
        ),
    );
    
    Ok(NodesConfig {
        peers,
        this,
        file_dir: ".".to_string(),
    })
}

// 读取配置文件
fn read_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&content)?;
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    // 解析命令行参数
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: {} <配置文件> <节点名>", args[0]);
        std::process::exit(1);
    }
    
    let config_path = &args[1];
    let node_name = &args[2];
    
    info!("启动节点: {}", node_name);
    
    // 读取配置
    let config = read_config(config_path)?;
    info!("配置已加载: {:?}", config);
    
    // 构建节点配置
    let nodes_config = build_nodes_config(&config, node_name)?;
    
    // 创建系统
    let mut sys = Sys::new(nodes_config);
    
    // 获取LogicalModules的可变引用
    let lm_ref = sys.logical_modules.clone();
    let mut lm = lm_ref.lock().unwrap();
    
    if let Some(modules) = lm.as_mut() {
        // 注册模块类型
        let p2p_offset = register_module!(modules, P2PModule);
        info!("P2P模块注册成功，偏移量: {}", p2p_offset);
        
        let chat_offset = register_module!(modules, ChatNodeModule);
        info!("聊天模块注册成功，偏移量: {}", chat_offset);
        
        // 打印内存布局
        debug!("{}", modules.get_memory_layout());
        
        // 创建模块实例
        let args = LogicalModuleNewArgs {
            logical_modules_ref: tele_framework::sys::LogicalModulesRef::new(lm_ref.clone()),
            parent_name: "root".to_string(),
            btx: tokio::sync::broadcast::channel(10).0,
            logical_models: Some(std::sync::Arc::downgrade(&lm_ref)),
            nodes_config: nodes_config.clone(),
        };
        
        // 创建P2P模块并初始化
        let p2p = P2PModule::inner_new(args.clone());
        initialize_module!(modules, p2p).expect("初始化P2P模块失败");
        
        // 创建聊天模块并初始化
        let chat = ChatNodeModule::inner_new(args.clone());
        initialize_module!(modules, chat).expect("初始化聊天模块失败");
        
        // 再次打印内存布局（显示已初始化状态）
        debug!("{}", modules.get_memory_layout());
    }
    
    // 释放锁
    drop(lm);
    
    // 初始化模块
    if let Some(modules) = sys.logical_modules.lock().unwrap().as_ref() {
        if let Some(p2p) = unsafe { modules.get_module::<P2PModule>() } {
            p2p.init().await?;
        }
        
        if let Some(chat) = unsafe { modules.get_module::<ChatNodeModule>() } {
            chat.init().await?;
        }
        
        // 启动所有模块
        if let Some(p2p) = unsafe { modules.get_module::<P2PModule>() } {
            let handles = p2p.start().await?;
            // 将句柄添加到系统中
        }
        
        if let Some(chat) = unsafe { modules.get_module::<ChatNodeModule>() } {
            let handles = chat.start().await?;
            // 将句柄添加到系统中
        }
    }
    
    // 创建聊天模块视图，以便于交互
    let modules_ref = tele_framework::sys::LogicalModulesRef::new(sys.logical_modules.clone());
    let chat_view = ChatNodeView::new(modules_ref);
    
    // 启动命令行输入循环
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin).lines();
    
    println!("聊天已启动。输入消息发送，输入 /exit 退出。");
    
    while let Some(line) = reader.next_line().await? {
        if line.trim() == "/exit" {
            break;
        }
        
        // 通过视图访问聊天模块并发送消息
        let chat = chat_view.chat();
        if let Err(e) = chat.send_message(line).await {
            error!("发送消息失败: {:?}", e);
        }
    }
    
    info!("关闭中...");
    
    Ok(())
} 