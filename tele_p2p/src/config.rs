use std::collections::HashMap;
use std::net::SocketAddr;

use crate::NodeID;

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub addr: SocketAddr,
}

#[derive(Clone, Debug)]
pub struct NodesConfig {
    pub this: (NodeID, NodeConfig),
    pub peers: HashMap<NodeID, NodeConfig>,
}

impl NodesConfig {
    pub fn get_addr_by_id(&self, id: NodeID) -> Option<SocketAddr> {
        if id == self.this.0 {
            Some(self.this.1.addr)
        } else {
            self.peers.get(&id).map(|p| p.addr)
        }
    }
} 