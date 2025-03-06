use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::m_p2p::TaskId;
use crate::msg_pack::MsgPack;
use crate::result::P2PResult;
use crate::config::NodesConfig;
use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc::{self, Sender};

pub struct P2PQuicNode {
    shared: Arc<P2PQuicNodeShared>,
    locked: Arc<P2PQuicNodeLocked>,
    config: NodesConfig,
}

struct P2PQuicNodeShared {
    peer_conns: RwLock<HashMap<SocketAddr, Arc<Connection>>>,
}

struct P2PQuicNodeLocked {
    sub_tasks: Mutex<Vec<TaskId>>,
}

struct Connection {
    addr: SocketAddr,
    sender: Sender<Vec<u8>>,
}

impl P2PQuicNode {
    pub fn new(config: NodesConfig) -> Self {
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
        // 实现启动逻辑
        Ok(())
    }

    pub async fn send<M: MsgPack>(&self, addr: SocketAddr, msg: M) -> P2PResult<()> {
        let data = msg.encode_to_vec();
        let sender = {
            let conns = self.shared.peer_conns.read();
            if let Some(conn) = conns.get(&addr) {
                conn.sender.clone()
            } else {
                return Err(crate::result::P2PError::ConnectionError(
                    "Connection not found".to_string(),
                ));
            }
        };

        sender.send(data).await.map_err(|e| {
            crate::result::P2PError::ConnectionError(format!("Failed to send message: {}", e))
        })?;

        Ok(())
    }

    pub async fn reserve_peer_conn(&self, addr: SocketAddr) -> P2PResult<()> {
        let mut conns = self.shared.peer_conns.write();
        if !conns.contains_key(&addr) {
            let (tx, _rx) = mpsc::channel(100);
            conns.insert(addr, Arc::new(Connection {
                addr,
                sender: tx,
            }));
        }
        Ok(())
    }
}

async fn handle_connection(_conn: Arc<Connection>, mut rx: mpsc::Receiver<Vec<u8>>) {
    while let Some(data) = rx.recv().await {
        let all_count = data.len() / 1024 + 1;
        let mut head = vec![0u8; 8];
        head[0..4].copy_from_slice(&(all_count as u32).to_le_bytes());
        head[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
        
        for i in 0..all_count {
            let start = i * 1024;
            let end = (start + 1024).min(data.len());
            let chunk = &data[start..end];
            let _packet = [head.as_slice(), chunk].concat();
            // 发送数据包
            println!("Sending packet {} of {}", i + 1, all_count);
        }
    }
} 