use crate::config::NodesConfig;
use crate::m_p2p::{
    MsgId, NodeID, P2PKernel, P2pModule, P2pModuleView, P2pModuleViewTrait, TaskId,
};
use crate::msg_pack::MsgPack;
use crate::result::{P2PError, P2PResult};
use async_trait::async_trait;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use prost::bytes::Bytes;
use qp2p::{Connection, ConnectionIncoming, Endpoint, IncomingConnections, WireMsg};
use rand::Rng;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::{debug, error, info, warn};

pub struct P2PQuicNode {
    shared: Arc<P2PQuicNodeShared>,
    locked: Arc<P2PQuicNodeLocked>,
    config: NodesConfig,
    view: P2pModuleView,
}

struct P2PQuicNodeShared {
    peer_conns: DashMap<NodeID, Arc<ConnectionPool>>,
}

struct P2PQuicNodeLocked {
    sub_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

struct ConnectionHandle {
    active: AtomicBool,
    conn: Connection,
}

struct ConnectionPool {
    connections: RwLock<Vec<Arc<ConnectionHandle>>>,
}

impl ConnectionPool {
    fn new() -> Self {
        Self {
            connections: RwLock::new(Vec::new()),
        }
    }

    fn get_random_connection(&self) -> Option<Arc<ConnectionHandle>> {
        let conns = self.connections.read();
        let mut invalid_flags = vec![true; conns.len()];
        for (i, conn) in conns.iter().enumerate() {
            if conn.active.load(Ordering::SeqCst) {
                invalid_flags[i] = false;
            } else {
                tracing::error!("connection at idx {} is inactive", i);
            }
        }
        let mut rng = rand::thread_rng();
        loop {
            // if all flags are false, return None
            if invalid_flags.iter().all(|flag| *flag) {
                return None;
            }
            let idx = rng.gen_range(0..conns.len());
            if !invalid_flags[idx] {
                return Some(conns[idx].clone());
            }
        }
        // conns
        //     .iter()
        //     .find(|conn| conn.active.load(Ordering::SeqCst))
        //     .cloned()
    }

    /// return latest connection count
    fn add_connection(&self, conn: Connection) -> usize {
        let handle = Arc::new(ConnectionHandle {
            active: AtomicBool::new(true),
            conn,
        });
        // self.connections.write().push(handle);
        // scan connections and remove invalid connection
        let mut connwrite = self.connections.write();
        connwrite.retain(|conn| conn.active.load(Ordering::SeqCst));
        connwrite.push(handle);
        connwrite.len()
    }
}

// struct Connection {
//     id: usize,
//     addr: SocketAddr,
//     sender: Sender<Vec<u8>>,
// }

fn deserialize_msg_id_task_id(head: &[u8]) -> P2PResult<(MsgId, TaskId)> {
    let (msg_id, task_id) = bincode::deserialize::<(MsgId, TaskId)>(head).map_err(|err| {
        P2PError::DeserialMsgIdTaskIdFailed {
            err: err,
            context: "deserialize_msg_id_task_id".to_owned(),
        }
    })?;
    Ok((msg_id, task_id))
}
fn serialize_msg_id_task_id(msg_id: MsgId, task_id: TaskId) -> Vec<u8> {
    let mut head: Vec<u8> = bincode::serialize(&(msg_id, task_id)).unwrap();
    head.insert(0, head.len() as u8);
    head
}

impl P2PQuicNode {
    pub fn new(config: NodesConfig, view: P2pModuleView) -> Self {
        info!("创建P2PQuicNode，本节点ID: {}", config.this.0);
        println!(
            "----- P2PQuicNode::new 被调用! 节点ID: {} -----",
            config.this.0
        );
        Self {
            shared: Arc::new(P2PQuicNodeShared {
                peer_conns: DashMap::new(),
            }),
            locked: Arc::new(P2PQuicNodeLocked {
                sub_tasks: Mutex::new(Vec::new()),
            }),
            config,
            view,
        }
    }

    pub async fn start_internal(&self) -> P2PResult<()> {
        info!("启动P2PQuicNode，监听地址: {}", self.config.this.1.addr);

        let listen_addr = SocketAddr::new(
            IpAddr::from_str("0.0.0.0").unwrap(),
            self.config.this.1.addr.port(),
        );

        // create an endpoint for us to listen on and send from.
        let (endpoint, mut incoming_conns) = Endpoint::builder()
            .addr(listen_addr)
            .keep_alive_interval(Duration::from_millis(100))
            // timeout of send or receive or connect, bigger than interval
            .idle_timeout(2 * 1_000 /* 3600s = 1h */)
            .server()
            .map_err(|err| {
                tracing::error!("addr: {}, err: {}", listen_addr, err);
                P2PError::StartServerError(err)
            })?;

        for (peer_id, peer_config) in &self.config.peers {
            let peer_addr = peer_config.addr;
            info!("准备连接到节点: {} ({})", peer_id, peer_addr);

            let endpoint = endpoint.clone();
            let shared = self.shared.clone();
            let this_addr = self.config.this.1.addr;
            let view_clone = self.view.clone();
            let config_clone = self.config.clone();

            let task = tokio::spawn(async move {
                // let mut endpoint = Some(endpoint);
                loop {
                    Self::connect_to_peer(
                        &endpoint,
                        peer_addr,
                        this_addr,
                        shared.clone(),
                        &view_clone,
                        config_clone.clone(),
                    )
                    .await;
                    // endpoint = Some(reuse_endpoint);
                    // match res {
                    //     Ok(_) => {
                    //         debug!("成功连接到节点: {}", peer_addr);
                    //     }
                    //     Err(e) => {
                    //         warn!("连接到节点 {} 失败: {}，将在10秒后重试", peer_addr, e);
                    //     }
                    // };
                    tracing::info!("将在10秒后重试连接 {}", peer_addr);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            });

            self.locked.sub_tasks.lock().push(task);
        }

        let shared = self.shared.clone();
        let this_addr = self.config.this.1.addr;
        let view_clone = self.view.clone();
        let config_clone = self.config.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = Self::listen_for_connections(
                this_addr,
                shared,
                view_clone,
                config_clone,
                incoming_conns,
            )
            .await
            {
                error!("监听连接失败: {}", e);
            }
        });

        self.locked.sub_tasks.lock().push(task);

        Ok(())
    }

    // return endpoint to reuse
    async fn connect_to_peer(
        endpoint: &Endpoint,
        peer_addr: SocketAddr,
        this_addr: SocketAddr,
        shared: Arc<P2PQuicNodeShared>,
        view: &P2pModuleView,
        config: NodesConfig,
    ) {
        tracing::info!("try to connect to {}", peer_addr);
        let res = endpoint.connect_to(&peer_addr).await;
        match res {
            Ok((connection, incoming)) => {
                tracing::info!("connected to {}", peer_addr);
                let _ = connection
                    .send((
                        Bytes::new(),
                        Bytes::new(),
                        Bytes::from(this_addr.to_string()),
                    ))
                    .await;
                // send self addr
                Self::handle_connection(
                    peer_addr,
                    this_addr,
                    config,
                    &view,
                    shared.clone(),
                    connection,
                    incoming,
                )
                .await;
                // tracing::info!("handled conflict_connection {}", addr);
            }
            Err(e) => {
                tracing::warn!(
                    "connect to {} failed, error: {:?}, will retry",
                    peer_addr,
                    e
                );
            }
        }
    }

    async fn listen_for_connections(
        this_addr: SocketAddr,
        shared: Arc<P2PQuicNodeShared>,
        view: P2pModuleView,
        config: NodesConfig,
        mut incoming_conns: IncomingConnections,
    ) -> P2PResult<()> {
        info!("开始在 {} 监听连接", this_addr);

        tracing::info!("start listening for new connections on node {}", this_addr);
        loop {
            tokio::select! {
                next_incoming= incoming_conns.next() => {
                    if let Some((connection, mut incoming)) = next_incoming {
                        let res = incoming.next().await;

                        match res {
                            Ok(msg) => {
                                if let Some(WireMsg((_head, _, bytes))) = msg {
                                    let addr=String::from_utf8(bytes.to_vec()).unwrap().parse::<SocketAddr>().unwrap();
                                    // tracing::info!("recv connect from {}", addr);
                                    if view.p2p_module().find_peer_id(&addr).is_none(){
                                        tracing::warn!("recv connect from unknown peer {}", addr);
                                        continue;
                                    }
                                    // handle_conflict_connection(&view,&shared, &endpoint, connection, incoming);
                                    let config=config.clone();
                                    let view=view.clone();
                                    let shared=shared.clone();
                                    tokio::spawn(async move {
                                        Self::handle_connection(
                                            addr,this_addr,config,&view, shared, connection, incoming).await;
                                    });
                                }else{
                                    tracing::warn!("didn't recv head");
                                    continue;
                                }
                            }
                            Err(err)=>{
                                tracing::info!("recv head error {:?}",err);
                                continue;
                            }
                        }
                    }else{
                        break;
                    }
                }
            }
        }

        tracing::info!("监听连接结束");
        Ok(())
    }

    async fn handle_connection(
        peer_addr: SocketAddr,
        this_addr: SocketAddr,
        config: NodesConfig,
        view: &P2pModuleView,
        shared: Arc<P2PQuicNodeShared>,
        connection: Connection,
        mut incoming: ConnectionIncoming,
    ) {
        info!("处理来自 {} 的连接", peer_addr);

        let peer_node_id = find_peer_id_from_config(&config, peer_addr);

        let peer_node_id = match peer_node_id {
            Some(id) => {
                info!("找到peer_addr {} 对应的节点ID: {}", peer_addr, id);
                id
            }
            None => {
                error!("无法找到peer_addr {} 对应的节点ID", peer_addr);
                return;
            }
        };

        // add connection to connection pool
        let pool = shared
            .peer_conns
            .entry(peer_node_id)
            .or_insert(Arc::new(ConnectionPool::new()))
            .clone();
        let conn_count = pool.add_connection(connection);
        tracing::debug!(
            "add {} connection to pool, total count: {}",
            peer_node_id,
            conn_count
        );

        loop {
            let next_msg = incoming.next().await;
            match next_msg {
                Ok(Some(WireMsg((_, _, mut bytes)))) => {
                    let headlen = bytes.split_to(1)[0];
                    let head = bytes.split_to(headlen as usize);
                    match deserialize_msg_id_task_id(&head) {
                        Ok((msg_id, task_id)) => {
                            //返回结果未处理     曾俊
                            view.p2p_module()
                                .dispatch(peer_node_id, msg_id, task_id, bytes.into());
                            // .todo_handle("This part of the code needs to be implemented.");
                        }
                        Err(err) => {
                            tracing::warn!("incoming deserial head error: {:?}", err);
                        }
                    }
                    // dispatch msgs
                }
                Ok(None) => {
                    tracing::warn!("incoming no msg, connection end");
                    break;
                }
                Err(err) => {
                    tracing::warn!("incoming error: {:?}", err);
                    break;
                }
            }
        }

        info!("与节点 {} 的连接已关闭", peer_node_id);
    }

    // pub async fn send<M: MsgPack>(&self, addr: SocketAddr, msg: M) -> P2PResult<()> {
    //     let data = msg.encode_to_vec();
    //     debug!("发送消息到 {}，大小: {} 字节", addr, data.len());

    //     let sender = self.get_connection(addr).await?;

    //     sender
    //         .send((Bytes::new(), Bytes::new(), data.into()))
    //         .await
    //         .map_err(|e| {
    //             error!("发送消息失败: {}", e);
    //             P2PError::ConnectionError(format!("发送消息失败: {}", e))
    //         })?;

    //     Ok(())
    // }

    fn get_connection(&self, nodeid: NodeID) -> P2PResult<Arc<ConnectionHandle>> {
        let pool = match self.shared.peer_conns.get(&nodeid) {
            Some(pool) => pool.clone(),
            None => return Err(P2PError::NoConnectionReady { nodeid }),
        };
        // random select one
        if let Some(conn) = pool.get_random_connection() {
            Ok(conn.clone())
        } else {
            tracing::error!("get random connection failed");
            Err(P2PError::NoConnectionReady { nodeid })
        }
    }

    fn debug_connection_nodes_and_count(&self) {
        let mut nodes_2_count = HashMap::new();
        for item in self.shared.peer_conns.iter() {
            nodes_2_count.insert(*item.key(), item.value().connections.read().len());
        }
        tracing::debug!("当前连接节点和数量: {:?}", nodes_2_count);
    }
    // async fn get_connection(&self, addr: SocketAddr) -> P2PResult<Arc<Connection>> {
    //     let pool = {
    //         let conns = self.shared.peer_conns.read();
    //         conns
    //             .get(&addr)
    //             .cloned()
    //             .ok_or_else(|| P2PError::ConnectionError(format!("找不到节点 {} 的连接池", addr)))?
    //     };

    //     let conns = pool.connections.read();
    //     if conns.is_empty() {
    //         return Err(P2PError::ConnectionError(format!(
    //             "节点 {} 没有活跃连接",
    //             addr
    //         )));
    //     }

    //     let idx = pool.next_conn_index.fetch_add(1, Ordering::SeqCst) % conns.len();
    //     let conn = &conns[idx];

    //     Ok(conn.clone())
    // }

    // pub async fn reserve_peer_conn(&self, addr: SocketAddr) -> P2PResult<()> {
    //     let mut conns = self.shared.peer_conns.write();
    //     if !conns.contains_key(&addr) {
    //         debug!("为节点 {} 创建连接池", addr);
    //         conns.insert(
    //             addr,
    //             Arc::new(ConnectionPool {
    //                 addr,
    //                 connections: RwLock::new(Vec::new()),
    //                 next_conn_index: AtomicUsize::new(0),
    //                 active_count: AtomicUsize::new(0),
    //             }),
    //         );
    //     }
    //     Ok(())
    // }

    fn find_peer_id(&self, addr: SocketAddr) -> Option<NodeID> {
        for (id, node_config) in &self.config.peers {
            if node_config.addr == addr {
                return Some(*id);
            }
        }
        None
    }
}

impl Clone for P2PQuicNode {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            locked: self.locked.clone(),
            config: self.config.clone(),
            view: self.view.clone(),
        }
    }
}

#[async_trait]
impl P2PKernel for P2PQuicNode {
    // async fn send_for_response(&self, nodeid: NodeID, req_data: Vec<u8>) -> P2PResult<Vec<u8>> {
    //     let addr = match self.config.get_node_addr(nodeid) {
    //         Some(addr) => addr,
    //         None => {
    //             error!("找不到节点ID: {}", nodeid);
    //             return Err(P2PError::Other(format!("找不到节点ID: {}", nodeid)));
    //         }
    //     };

    //     debug!("发送请求到节点 {}，等待响应", nodeid);

    //     let sender = self.get_connection(addr).await?;

    //     sender.send(req_data).await.map_err(|e| {
    //         error!("发送请求失败: {}", e);
    //         P2PError::ConnectionError(format!("发送请求失败: {}", e))
    //     })?;

    //     let (tx, rx) = tokio::sync::oneshot::channel();

    //     match tokio::time::timeout(Duration::from_secs(30), rx).await {
    //         Ok(Ok(resp)) => Ok(resp),
    //         Ok(Err(_)) => {
    //             error!("响应通道错误");
    //             Err(P2PError::Other("响应通道错误".to_string()))
    //         }
    //         Err(_) => {
    //             error!("等待响应超时");
    //             Err(P2PError::Timeout("等待响应超时".to_string()))
    //         }
    //     }
    // }

    async fn send(
        &self,
        node: NodeID,
        task_id: TaskId,
        msg_id: MsgId,
        req_data: Vec<u8>,
    ) -> P2PResult<()> {
        // let addr = match self.config.get_node_addr(node) {
        //     Some(addr) => addr,
        //     None => {
        //         error!("找不到节点ID: {}", node);
        //         return Err(P2PError::Other(format!("找不到节点ID: {}", node)));
        //     }
        // };

        debug!(
            "quic kernel {} 发送消息到节点 {}，任务ID: {}，消息ID: {}",
            self.config.this.0, node, task_id, msg_id
        );

        let msg_bytes = unsafe {
            let dataref = req_data.as_ptr();
            // transfer to static slice
            let data = std::slice::from_raw_parts::<'static>(dataref, req_data.len());
            let mut v = serialize_msg_id_task_id(msg_id, task_id);
            v.extend_from_slice(data);
            Bytes::from(v)
        };
        // let mut header = Vec::with_capacity(8);
        // header.extend_from_slice(&msg_id.to_le_bytes());
        // header.extend_from_slice(&task_id.to_le_bytes());

        // let mut message = Vec::with_capacity(header.len() + req_data.len());
        // message.extend_from_slice(&header);
        // message.extend_from_slice(&req_data);

        let sender = match self.get_connection(node) {
            Err(err) => {
                tracing::error!("获取连接失败: {}", err);
                self.debug_connection_nodes_and_count();
                return Err(err);
            }
            Ok(sender) => sender,
        };

        if let Err(err) = sender
            .conn
            .send((Bytes::new(), Bytes::new(), msg_bytes))
            .await
        {
            tracing::error!("发送消息失败: {}", err);
            // set connection to inactive
            sender.active.store(false, Ordering::SeqCst);
        }

        Ok(())
    }

    async fn start(&self) -> P2PResult<()> {
        info!("P2PKernel::start - 调用P2PQuicNode::start_internal");
        self.start_internal().await
    }
}

fn find_peer_id_from_config(config: &NodesConfig, addr: SocketAddr) -> Option<NodeID> {
    for (id, node_config) in &config.peers {
        if node_config.addr == addr {
            return Some(*id);
        }
    }
    None
}
