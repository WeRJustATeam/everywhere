use std::collections::HashMap;
use std::net::{SocketAddr, IpAddr};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::str::FromStr;
use std::time::Duration;
use crate::m_p2p::{TaskId, MsgId, NodeID, P2PKernel};
use crate::msg_pack::MsgPack;
use crate::result::{P2PResult, P2PError};
use crate::config::NodesConfig;
use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc::{self, Sender, Receiver};
use async_trait::async_trait;
use tracing::{debug, error, info, warn};
use prost::bytes::Bytes;

pub struct P2PQuicNode {
    shared: Arc<P2PQuicNodeShared>,
    locked: Arc<P2PQuicNodeLocked>,
    config: NodesConfig,
}

struct P2PQuicNodeShared {
    peer_conns: RwLock<HashMap<SocketAddr, Arc<ConnectionPool>>>,
}

struct P2PQuicNodeLocked {
    sub_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

struct ConnectionPool {
    addr: SocketAddr,
    connections: RwLock<Vec<Connection>>,
    next_conn_index: AtomicUsize,
    active_count: AtomicUsize,
}

struct Connection {
    id: usize,
    addr: SocketAddr,
    sender: Sender<Vec<u8>>,
}

impl P2PQuicNode {
    pub fn new(config: NodesConfig) -> Self {
        info!("创建P2PQuicNode，本节点ID: {}", config.this.0);
        Self {
            shared: Arc::new(P2PQuicNodeShared {
                peer_conns: RwLock::new(HashMap::new()),
            }),
            locked: Arc::new(P2PQuicNodeLocked {
                sub_tasks: Mutex::new(Vec::new()),
            }),
            config,
        }
    }

    pub async fn start(&self) -> P2PResult<()> {
        info!("启动P2PQuicNode，监听地址: {}", self.config.this.1.addr);
        
        // 创建监听端点
        let listen_addr = SocketAddr::new(
            IpAddr::from_str("0.0.0.0").unwrap(),
            self.config.this.1.addr.port()
        );
        
        // 为每个对等节点创建连接
        for (peer_id, peer_config) in &self.config.peers {
            let peer_addr = peer_config.addr;
            info!("准备连接到节点: {} ({})", peer_id, peer_addr);
            
            // 创建连接池
            self.reserve_peer_conn(peer_addr).await?;
            
            // 启动连接任务
            let shared = self.shared.clone();
            let this_addr = self.config.this.1.addr;
            let task = tokio::spawn(async move {
                loop {
                    match Self::connect_to_peer(peer_addr, this_addr, shared.clone()).await {
                        Ok(_) => {
                            debug!("成功连接到节点: {}", peer_addr);
                        }
                        Err(e) => {
                            warn!("连接到节点 {} 失败: {}，将在10秒后重试", peer_addr, e);
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });
            
            self.locked.sub_tasks.lock().push(task);
        }
        
        // 启动监听任务
        let shared = self.shared.clone();
        let this_addr = self.config.this.1.addr;
        let task = tokio::spawn(async move {
            if let Err(e) = Self::listen_for_connections(listen_addr, this_addr, shared).await {
                error!("监听连接失败: {}", e);
            }
        });
        
        self.locked.sub_tasks.lock().push(task);
        
        Ok(())
    }
    
    async fn connect_to_peer(
        peer_addr: SocketAddr,
        this_addr: SocketAddr,
        shared: Arc<P2PQuicNodeShared>,
    ) -> P2PResult<()> {
        info!("尝试连接到节点: {}", peer_addr);
        
        // 这里应该实现实际的QUIC连接
        // 为了简化，我们创建一个模拟连接
        let (tx, rx) = mpsc::channel(100);
        
        let conn = Connection {
            id: rand::random(),
            addr: peer_addr,
            sender: tx,
        };
        
        // 获取连接池
        let pool = {
            let conns = shared.peer_conns.read();
            conns.get(&peer_addr).cloned().ok_or_else(|| {
                P2PError::ConnectionError(format!("找不到节点 {} 的连接池", peer_addr))
            })?
        };
        
        // 添加连接到池中
        {
            let mut conns = pool.connections.write();
            conns.push(conn);
            pool.active_count.fetch_add(1, Ordering::SeqCst);
            debug!("添加连接到池中，当前活跃连接数: {}", pool.active_count.load(Ordering::SeqCst));
        }
        
        // 启动接收任务
        tokio::spawn(async move {
            Self::handle_connection(peer_addr, this_addr, rx).await;
        });
        
        Ok(())
    }
    
    async fn listen_for_connections(
        listen_addr: SocketAddr,
        this_addr: SocketAddr,
        shared: Arc<P2PQuicNodeShared>,
    ) -> P2PResult<()> {
        info!("开始监听连接: {}", listen_addr);
        
        // 这里应该实现实际的QUIC监听
        // 为了简化，我们只是模拟监听
        
        // 无限循环，模拟持续监听
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            debug!("监听中...");
        }
    }
    
    async fn handle_connection(
        peer_addr: SocketAddr,
        this_addr: SocketAddr,
        mut rx: Receiver<Vec<u8>>,
    ) {
        info!("处理来自 {} 的连接", peer_addr);
        
        while let Some(data) = rx.recv().await {
            debug!("收到来自 {} 的数据，大小: {} 字节", peer_addr, data.len());
            
            // 这里应该解析消息并分发
            // 为了简化，我们只是记录日志
            
            // 解析消息头
            if data.len() < 8 {
                warn!("收到的数据太短，无法解析消息头");
                continue;
            }
            
            // 模拟消息处理
            debug!("处理消息完成");
        }
        
        info!("与 {} 的连接已关闭", peer_addr);
    }

    pub async fn send<M: MsgPack>(&self, addr: SocketAddr, msg: M) -> P2PResult<()> {
        let data = msg.encode_to_vec();
        debug!("发送消息到 {}，大小: {} 字节", addr, data.len());
        
        let sender = self.get_connection(addr).await?;
        
        sender.send(data).await.map_err(|e| {
            error!("发送消息失败: {}", e);
            P2PError::ConnectionError(format!("发送消息失败: {}", e))
        })?;
        
        Ok(())
    }
    
    async fn get_connection(&self, addr: SocketAddr) -> P2PResult<Sender<Vec<u8>>> {
        let pool = {
            let conns = self.shared.peer_conns.read();
            conns.get(&addr).cloned().ok_or_else(|| {
                P2PError::ConnectionError(format!("找不到节点 {} 的连接池", addr))
            })?
        };
        
        let conns = pool.connections.read();
        if conns.is_empty() {
            return Err(P2PError::ConnectionError(format!("节点 {} 没有活跃连接", addr)));
        }
        
        // 使用轮询方式选择连接
        let idx = pool.next_conn_index.fetch_add(1, Ordering::SeqCst) % conns.len();
        let conn = &conns[idx];
        
        Ok(conn.sender.clone())
    }

    pub async fn reserve_peer_conn(&self, addr: SocketAddr) -> P2PResult<()> {
        let mut conns = self.shared.peer_conns.write();
        if !conns.contains_key(&addr) {
            debug!("为节点 {} 创建连接池", addr);
            conns.insert(addr, Arc::new(ConnectionPool {
                addr,
                connections: RwLock::new(Vec::new()),
                next_conn_index: AtomicUsize::new(0),
                active_count: AtomicUsize::new(0),
            }));
        }
        Ok(())
    }
}

#[async_trait]
impl P2PKernel for P2PQuicNode {
    async fn send_for_response(&self, nodeid: NodeID, req_data: Vec<u8>) -> P2PResult<Vec<u8>> {
        let addr = match self.config.get_node_addr(nodeid) {
            Some(addr) => addr,
            None => {
                error!("找不到节点ID: {}", nodeid);
                return Err(P2PError::Other(format!("找不到节点ID: {}", nodeid)));
            }
        };
        
        debug!("发送请求到节点 {}，等待响应", nodeid);
        
        // 发送请求
        let sender = self.get_connection(addr).await?;
        
        sender.send(req_data).await.map_err(|e| {
            error!("发送请求失败: {}", e);
            P2PError::ConnectionError(format!("发送请求失败: {}", e))
        })?;
        
        // 创建一个响应通道
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        // 在实际实现中，我们应该注册这个通道以接收响应
        // 为了简化，我们直接返回一个空响应
        
        // 等待响应，带超时
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                error!("响应通道错误");
                Err(P2PError::Other("响应通道错误".to_string()))
            },
            Err(_) => {
                error!("等待响应超时");
                Err(P2PError::Timeout("等待响应超时".to_string()))
            }
        }
    }
    
    async fn send(
        &self,
        node: NodeID,
        task_id: TaskId,
        msg_id: MsgId,
        req_data: Vec<u8>,
    ) -> P2PResult<()> {
        let addr = match self.config.get_node_addr(node) {
            Some(addr) => addr,
            None => {
                error!("找不到节点ID: {}", node);
                return Err(P2PError::Other(format!("找不到节点ID: {}", node)));
            }
        };
        
        debug!("发送消息到节点 {}，任务ID: {}，消息ID: {}", node, task_id, msg_id);
        
        // 构建消息头
        let mut header = Vec::with_capacity(8);
        header.extend_from_slice(&msg_id.to_le_bytes());
        header.extend_from_slice(&task_id.to_le_bytes());
        
        // 构建完整消息
        let mut message = Vec::with_capacity(header.len() + req_data.len());
        message.extend_from_slice(&header);
        message.extend_from_slice(&req_data);
        
        // 发送消息
        let sender = self.get_connection(addr).await?;
        
        sender.send(message).await.map_err(|e| {
            error!("发送消息失败: {}", e);
            P2PError::ConnectionError(format!("发送消息失败: {}", e))
        })?;
        
        Ok(())
    }
} 