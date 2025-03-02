use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    WebSocketStream, MaybeTlsStream
};
use url::Url;
use uuid::Uuid;
use tokio::io::{self, AsyncBufReadExt};
use std::fs;

// 定义ANSI颜色代码常量
const COLOR_RESET: &str = "\x1b[0m";
const COLOR_RED: &str = "\x1b[31m";
const COLOR_GREEN: &str = "\x1b[32m";
const COLOR_YELLOW: &str = "\x1b[33m";
const COLOR_BLUE: &str = "\x1b[34m";
const COLOR_MAGENTA: &str = "\x1b[35m";
const COLOR_CYAN: &str = "\x1b[36m";
const COLOR_WHITE: &str = "\x1b[37m";

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
    nodes: HashMap<String, NodeConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NodeConfig {
    bind_address: String,
}

// 定义 WebSocket 流类型
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = futures_util::stream::SplitSink<WsStream, Message>;
type WsSource = futures_util::stream::SplitStream<WsStream>;

// 发送通道
#[derive(Clone)]
struct PeerSender {
    addr: String,
    sender: mpsc::Sender<ChatMessage>,
}

// 定义消息类型
#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    id: String,
    sender: String,
    content: String,
    timestamp: u64,
}

// 定义节点状态
struct Node {
    id: String,
    peers: HashMap<String, PeerSender>,
    seen_messages: HashSet<String>,
    messages: Vec<ChatMessage>,
}

impl Node {
    fn new(id: String) -> Self {
        Node {
            id,
            peers: HashMap::new(),
            seen_messages: HashSet::new(),
            messages: Vec::new(),
        }
    }
}

// 读取配置文件
fn read_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string(path)?;
    let config: Config = serde_yaml::from_str(&config_str)?;
    Ok(config)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 设置日志
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug");
    }
    env_logger::init();
    
    // 获取命令行参数：配置文件路径和节点ID
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: {} <配置文件路径> <节点ID>", args[0]);
        return Err("参数不足".into());
    }
    
    let config_path = &args[1];
    let node_id = &args[2];
    
    // 读取配置
    debug!("读取配置文件: {}", config_path);
    let config = match read_config(config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            error!("读取配置文件失败: {}", e);
            return Err(e);
        }
    };
    
    // 检查节点ID是否存在
    if !config.nodes.contains_key(node_id) {
        error!("配置文件中不存在节点ID: {}", node_id);
        return Err(format!("节点ID不存在: {}", node_id).into());
    }
    
    let node_config = config.nodes.get(node_id).unwrap().clone();
    debug!("节点ID: {}", node_id);
    debug!("绑定地址: {}", node_config.bind_address);
    
    // 获取其他节点地址
    let peers: Vec<String> = config.nodes.iter()
        .filter(|(id, _)| *id != node_id)
        .map(|(_, cfg)| cfg.bind_address.clone())
        .collect();
    
    debug!("对等节点: {:?}", peers);
    
    // 创建节点
    let node = Arc::new(Mutex::new(Node::new(node_id.clone())));
    
    // 启动 WebSocket 服务器
    let listener = TcpListener::bind(&node_config.bind_address).await?;
    info!("WebSocket 服务器启动于 {}", node_config.bind_address);

    let node_clone = node.clone();
    
    // 处理入站连接
    tokio::spawn(async move {
        while let Ok((stream, addr)) = listener.accept().await {
            let addr_str = addr.to_string();
            info!("新的入站连接: {} ({})", addr_str, addr);

            let node_for_handler = node_clone.clone();
            tokio::spawn(async move {
                // 使用MaybeTlsStream包装TcpStream
                let stream = MaybeTlsStream::Plain(stream);
                match tokio_tungstenite::accept_async(stream).await {
                    Ok(ws_stream) => {
                        // 使用split分离读写流
                        let (ws_sink, ws_source) = ws_stream.split();
                        
                        // 创建消息发送通道
                        let (tx, rx) = mpsc::channel::<ChatMessage>(100);
                        let peer_sender = PeerSender {
                            addr: addr_str.clone(),
                            sender: tx,
                        };
                        
                        {
                            debug!("获取节点锁以添加新连接");
                            let mut node = node_for_handler.lock().await;
                            debug!("成功获取节点锁");
                            
                            // 使用网络地址的字符串形式作为键
                            node.peers.insert(addr_str.clone(), peer_sender);
                            
                            // 打印peers列表信息
                            let peers_count = node.peers.len();
                            let peers_list = node.peers.keys().cloned().collect::<Vec<_>>();
                            debug!("已将入站节点 {} 添加到peer列表，当前共有 {} 个peers: {:?}", addr_str, peers_count, peers_list);
                        }
                        
                        // 启动独立的发送任务
                        let sink_task = handle_sink_task(ws_sink, rx, addr_str.clone());
                        
                        // 启动独立的接收任务
                        let source_task = handle_source_task(node_for_handler.clone(), ws_source, addr_str.clone());
                        
                        // 等待任意一个任务完成（意味着连接已断开）
                        tokio::select! {
                            _ = sink_task => {
                                info!("发送任务结束，连接断开: {}", addr_str);
                            }
                            _ = source_task => {
                                info!("接收任务结束，连接断开: {}", addr_str);
                            }
                        }
                        
                        // 连接断开，清理资源
                        info!("节点 {} 断开连接，开始清理资源", addr_str);
                        let mut node = node_for_handler.lock().await;
                        node.peers.remove(&addr_str);
                        
                        // 打印剩余的连接
                        let peers_list = node.peers.keys().cloned().collect::<Vec<_>>();
                        debug!("已从peer列表移除节点 {}，当前剩余peers: {:?}", addr_str, peers_list);
                    }
                    Err(e) => error!("WebSocket 握手失败: {}", e),
                }
            });
        }
    });

    // 连接到配置中的所有对等节点
    for peer_addr in &peers {
        connect_to_peer(&node, peer_addr).await;
    }

    // 添加用户输入处理
    let node_for_input = node.clone();
    let node_id = node_id.clone();
    tokio::spawn(async move {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin).lines();
        
        println!("{}聊天已启动，输入消息内容后按回车发送 (输入 'exit' 退出){}", COLOR_GREEN, COLOR_RESET);
        println!("\n{}>>{} ", COLOR_CYAN, COLOR_RESET); 
        
        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim() == "exit" {
                println!("{}正在退出程序...{}", COLOR_RED, COLOR_RESET);
                std::process::exit(0);
            }
            
            // 创建消息
            let message = ChatMessage {
                id: format!("msg-{}", Uuid::new_v4()),
                sender: node_id.clone(),
                content: line.trim().to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };
            
            // 使用克隆的消息对象，避免阻塞用户输入线程
            let message_clone = message.clone();
            let node_clone = node_for_input.clone();
            tokio::spawn(async move {
                // 在新的异步任务中处理消息，避免阻塞用户输入
                handle_message(node_clone, message_clone, "LOCAL".to_string()).await;
            });
            
            // 添加消息提示音（控制台蜂鸣声）
            print!("\x07");
            
            // 显示输入提示
            println!("\n{}>>{} ", COLOR_CYAN, COLOR_RESET);
        }
    });
    
    // 保持程序运行
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn connect_to_peer(node: &Arc<Mutex<Node>>, addr: &str) {
    debug!("========== 开始连接到对等节点: {} ==========", addr);
    debug!("尝试连接到节点: {}", addr);
    let url = Url::parse(&format!("ws://{}", addr)).expect("无效的 URL");
    
    // 使用重试机制尝试连接
    let max_retries = 3;
    let mut retry_count = 0;
    let retry_delay = std::time::Duration::from_secs(2);
    let connect_id = format!("connect-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("unknown"));
    
    debug!("[连接器-{}] 开始连接到 {}", connect_id, addr);
    
    while retry_count < max_retries {
        debug!("[连接器-{}] 尝试 #{} 连接到 {}", connect_id, retry_count + 1, addr);
        match connect_async(url.clone()).await {
            Ok((ws_stream, _)) => {
                info!("[连接器-{}] 成功连接到节点: {}", connect_id, addr);
                
                // 分离WebSocket流的读写部分
                let (ws_sink, ws_source) = ws_stream.split();
                
                // 创建消息发送通道
                let (tx, rx) = mpsc::channel::<ChatMessage>(100);
                let peer_sender = PeerSender {
                    addr: addr.to_string(),
                    sender: tx,
                };
                
                {
                    debug!("[连接器-{}] 获取节点锁以添加新连接", connect_id);
                    let mut node_guard = node.lock().await;
                    debug!("[连接器-{}] 成功获取节点锁", connect_id);
                    
                    // 保存时使用实际地址而不是连接URL
                    node_guard.peers.insert(addr.to_string(), peer_sender);
                    
                    // 打印peers列表信息
                    let peers_count = node_guard.peers.len();
                    let peers_list = node_guard.peers.keys().cloned().collect::<Vec<_>>();
                    debug!("[连接器-{}] 已将节点 {} 添加到peer列表，当前共有 {} 个peers: {:?}", 
                         connect_id, addr, peers_count, peers_list);
                }

                // 启动独立的发送和接收任务
                let node_clone = Arc::clone(node);
                let addr_clone = addr.to_string();
                
                debug!("[连接器-{}] 创建处理连接的异步任务", connect_id);
                tokio::spawn(async move {
                    let sink_task = handle_sink_task(ws_sink, rx, addr_clone.clone());
                    let source_task = handle_source_task(node_clone.clone(), ws_source, addr_clone.clone());
                    
                    // 等待任意一个任务完成（意味着连接已断开）
                    tokio::select! {
                        _ = sink_task => {
                            info!("发送任务结束，连接断开: {}", addr_clone);
                        }
                        _ = source_task => {
                            info!("接收任务结束，连接断开: {}", addr_clone);
                        }
                    }
                    
                    // 连接断开，清理资源
                    info!("节点 {} 断开连接，开始清理资源", addr_clone);
                    let mut node = node_clone.lock().await;
                    node.peers.remove(&addr_clone);
                    
                    // 打印剩余的连接
                    let peers_list = node.peers.keys().cloned().collect::<Vec<_>>();
                    debug!("已从peer列表移除节点 {}，当前剩余peers: {:?}", addr_clone, peers_list);
                });
                
                // 连接成功，退出函数
                debug!("[连接器-{}] 连接过程完成，退出connect_to_peer函数", connect_id);
                debug!("========== 节点连接完成: {} ==========", addr);
                return;
            }
            Err(e) => {
                retry_count += 1;
                error!("[连接器-{}] 连接到节点 {} 失败 (尝试 {}/{}): {}", 
                      connect_id, addr, retry_count, max_retries, e);
                
                if retry_count < max_retries {
                    debug!("[连接器-{}] 将在 {}秒 后重试连接到节点: {}", 
                         connect_id, retry_delay.as_secs(), addr);
                    tokio::time::sleep(retry_delay).await;
                } else {
                    debug!("[连接器-{}] 连接到节点 {} 的所有尝试均失败，将在后台继续尝试连接", 
                          connect_id, addr);
                    
                    // 启动一个后台任务，持续尝试连接
                    let node_clone = Arc::clone(node);
                    let addr_clone = addr.to_string();
                    let connect_id_clone = connect_id.clone();
                    debug!("[连接器-{}] 创建后台连接任务", connect_id);
                    tokio::spawn(async move {
                        let backoff_delay = std::time::Duration::from_secs(5);
                        let mut attempt_count = 0;
                        loop {
                            attempt_count += 1;
                            debug!("[连接器-{}] 后台尝试 #{} 连接到节点: {}", 
                                 connect_id_clone, attempt_count, addr_clone);
                            match connect_async(Url::parse(&format!("ws://{}", addr_clone)).unwrap()).await {
                                Ok((ws_stream, _)) => {
                                    info!("[连接器-{}] 后台任务成功连接到节点: {}", 
                                         connect_id_clone, addr_clone);
                                    
                                    // 分离WebSocket流的读写部分
                                    let (ws_sink, ws_source) = ws_stream.split();
                                    
                                    // 创建消息发送通道
                                    let (tx, rx) = mpsc::channel::<ChatMessage>(100);
                                    let peer_sender = PeerSender {
                                        addr: addr_clone.clone(),
                                        sender: tx,
                                    };
                                    
                                    {
                                        debug!("[连接器-{}] 获取节点锁以添加新连接", connect_id_clone);
                                        let mut node_guard = node_clone.lock().await;
                                        debug!("[连接器-{}] 成功获取节点锁", connect_id_clone);
                                        node_guard.peers.insert(addr_clone.clone(), peer_sender);
                                        let peers_list = node_guard.peers.keys().cloned().collect::<Vec<_>>();
                                        debug!("[连接器-{}] 已将节点 {} 添加到peer列表，当前peers: {:?}", 
                                             connect_id_clone, addr_clone, peers_list);
                                    }
                                    
                                    let sink_task = handle_sink_task(ws_sink, rx, addr_clone.clone());
                                    let source_task = handle_source_task(node_clone.clone(), ws_source, addr_clone.clone());
                                    
                                    // 等待任意一个任务完成
                                    tokio::select! {
                                        _ = sink_task => {
                                            info!("发送任务结束，连接断开: {}", addr_clone);
                                        }
                                        _ = source_task => {
                                            info!("接收任务结束，连接断开: {}", addr_clone);
                                        }
                                    }
                                    
                                    // 连接断开，清理资源
                                    info!("节点 {} 断开连接，开始清理资源", addr_clone);
                                    let mut node = node_clone.lock().await;
                                    node.peers.remove(&addr_clone);
                                    
                                    debug!("[连接器-{}] 连接处理结束", connect_id_clone);
                                    break;
                                }
                                Err(e) => {
                                    error!("[连接器-{}] 后台连接到节点 {} 失败: {}", 
                                          connect_id_clone, addr_clone, e);
                                    debug!("[连接器-{}] 等待 {}秒 后重试", 
                                         connect_id_clone, backoff_delay.as_secs());
                                    tokio::time::sleep(backoff_delay).await;
                                }
                            }
                        }
                    });
                    debug!("[连接器-{}] 后台连接任务已创建", connect_id);
                }
            }
        }
    }
    debug!("[连接器-{}] 连接过程完成", connect_id);
    debug!("========== 节点连接过程结束: {} ==========", addr);
}

// 处理WebSocket发送通道（写入部分）
async fn handle_sink_task(
    mut ws_sink: WsSink,
    mut rx: mpsc::Receiver<ChatMessage>,
    peer_addr: String
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let connection_id = format!("sink-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("unknown"));
    debug!("[发送器-{}] 启动发送任务处理节点 {}", connection_id, peer_addr);
    
    // 处理发送通道接收到的消息
    while let Some(message) = rx.recv().await {
        debug!("[发送器-{}] 接收到要发送的消息: {}", connection_id, message.id);
        
        // 序列化消息
        match serde_json::to_string(&message) {
            Ok(json) => {
                debug!("[发送器-{}] 消息序列化成功，准备发送", connection_id);
                
                // 发送消息
                if let Err(e) = ws_sink.send(Message::Text(json)).await {
                    error!("[发送器-{}] 发送消息失败: {}", connection_id, e);
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("发送消息失败: {}", e)
                    )));
                }
                
                info!("[发送器-{}] 消息发送成功: {} 内容: \"{}\"", 
                     connection_id, message.id, message.content);
            }
            Err(e) => {
                error!("[发送器-{}] 序列化消息失败: {}", connection_id, e);
            }
        }
    }
    
    debug!("[发送器-{}] 发送通道已关闭，结束发送任务", connection_id);
    Ok(())
}

// 处理WebSocket接收通道（读取部分）
async fn handle_source_task(
    node: Arc<Mutex<Node>>,
    mut ws_source: WsSource,
    peer_addr: String
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let connection_id = format!("source-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("unknown"));
    debug!("[接收器-{}] 启动接收任务处理节点 {}", connection_id, peer_addr);
    
    // 无限循环等待消息
    while let Some(msg_result) = ws_source.next().await {
        match msg_result {
            Ok(msg) => {
                debug!("[接收器-{}] 收到WebSocket消息", connection_id);
                match msg {
                    Message::Text(text) => {
                        debug!("[接收器-{}] 收到文本消息，长度: {} 字节", connection_id, text.len());
                        if let Ok(message) = serde_json::from_str::<ChatMessage>(&text) {
                            info!("[接收器-{}] 成功接收消息: id={}, sender={}, 内容: \"{}\"", 
                                 connection_id, message.id, message.sender, message.content);
                            // 处理消息
                            handle_message(node.clone(), message, peer_addr.clone()).await;
                        } else {
                            error!("[接收器-{}] 无法解析来自节点 {} 的消息: {}", connection_id, peer_addr, text);
                        }
                    }
                    Message::Close(_) => {
                        debug!("[接收器-{}] 节点 {} 关闭连接", connection_id, peer_addr);
                        break;
                    }
                    Message::Ping(_) => {
                        debug!("[接收器-{}] 从节点 {} 收到Ping消息", connection_id, peer_addr);
                    }
                    Message::Pong(_) => {
                        debug!("[接收器-{}] 从节点 {} 收到Pong消息", connection_id, peer_addr);
                    }
                    Message::Binary(_) => {
                        debug!("[接收器-{}] 从节点 {} 收到二进制消息", connection_id, peer_addr);
                    }
                    Message::Frame(_) => {
                        debug!("[接收器-{}] 从节点 {} 收到帧消息", connection_id, peer_addr);
                    }
                }
            }
            Err(e) => {
                error!("[接收器-{}] 从节点 {} 接收消息出错: {}", connection_id, peer_addr, e);
                break;
            }
        }
    }
    
    debug!("[接收器-{}] 接收循环结束，节点连接已断开: {}", connection_id, peer_addr);
    Ok(())
}

async fn handle_message(node: Arc<Mutex<Node>>, message: ChatMessage, source_addr: String) {
    debug!("========== 开始处理消息 ==========");
    debug!("消息ID: {}, 发送者: {}, 源地址: {}", message.id, message.sender, source_addr);
    
    // 检查消息ID是否已处理过
    let (is_new_message, peers_to_send) = {
        debug!("步骤1: 获取节点锁，检查消息是否已处理");
        let mut node_guard = node.lock().await;
        debug!("成功获取节点锁");
        
        let is_new = !node_guard.seen_messages.contains(&message.id);
        debug!("消息是否新消息: {}", is_new);
        
        // 如果是新消息，添加到已处理消息集合中
        if is_new {
            debug!("步骤2: 处理新消息 - 添加到已处理集合");
            node_guard.seen_messages.insert(message.id.clone());
            debug!("已将消息ID添加到seen_messages集合，当前集合大小: {}", node_guard.seen_messages.len());
            
            node_guard.messages.push(message.clone());
            debug!("已将消息添加到messages历史，当前历史大小: {}", node_guard.messages.len());
            
            // 获取发送者的颜色
            let sender_color = get_color_for_sender(&message.sender);
            
            // 记录消息到info日志
            info!("收到消息: 发送者={}, 内容=\"{}\"", message.sender, message.content);
            
            // 增强版打印消息内容
            println!("\n┌────────────────────────────────────────────────");
            println!("│ 时间: [{}]", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
            println!("│ 发送者: {}{}{}", sender_color, message.sender, COLOR_RESET);
            println!("│ 消息ID: {}", message.id);
            println!("├────────────────────────────────────────────────");
            println!("│ {}{}{}", sender_color, message.content, COLOR_RESET);
            println!("└────────────────────────────────────────────────");
            
            // 添加消息提示音（控制台蜂鸣声）
            print!("\x07");
            
            // 显示输入提示
            println!("\n{}>>{} ", COLOR_CYAN, COLOR_RESET);
        } else {
            debug!("步骤2: 忽略已处理的消息");
        }
        
        // 获取所有对等节点
        debug!("步骤3: 确定需要广播的目标节点");
        let filtered_peers = if is_new {
            let peers: Vec<_> = node_guard.peers.values()
                .filter(|peer| peer.addr != source_addr)
                .cloned()
                .collect();
            
            debug!("已过滤出 {} 个目标节点（排除源节点）", peers.len());
            
            // 打印每个目标节点
            for (i, peer) in peers.iter().enumerate() {
                debug!("目标节点 #{}: {}", i+1, peer.addr);
            }
            
            peers
        } else {
            debug!("已处理消息，无需广播");
            Vec::new()
        };
        
        if is_new {
            debug!("准备将消息广播到 {} 个节点（排除源节点 {}）", filtered_peers.len(), source_addr);
        }
        
        (is_new, filtered_peers)
    };
    
    if !is_new_message {
        debug!("========== 消息处理结束（已忽略重复消息）==========");
        return;
    }
    
    debug!("步骤4: 开始广播消息");
    debug!("开始广播消息: id={}, content={}, 目标节点数={}", message.id, message.content, peers_to_send.len());
    
    // 将消息发送到每个对等节点的发送通道
    for peer in peers_to_send {
        let message_clone = message.clone();
        
        match peer.sender.send(message_clone.clone()).await {
            Ok(_) => {
                info!("已将消息发送到节点 {}", peer.addr);
            }
            Err(e) => {
                error!("无法向节点 {} 发送消息: {}", peer.addr, e);
            }
        }
    }
    
    debug!("消息广播完成: id={}", message.id);
    debug!("========== 消息处理完成 ==========\n");
} 