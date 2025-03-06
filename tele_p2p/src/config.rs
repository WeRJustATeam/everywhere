use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::m_p2p::NodeID;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    pub addr: SocketAddr,
    pub spec: String,
    pub cert: Vec<u8>,
    pub priv_key: Vec<u8>,
}

impl NodeConfig {
    pub fn is_master(&self) -> bool {
        self.spec.contains("master")
    }

    pub fn is_worker(&self) -> bool {
        self.spec.contains("worker")
    }
}

#[derive(Clone, Debug)]
pub struct NodesConfig {
    pub this: (NodeID, NodeConfig),
    pub peers: HashMap<NodeID, NodeConfig>,
    pub file_dir: PathBuf,
}

impl NodesConfig {
    pub fn new(this_id: NodeID, nodes: Vec<(NodeID, SocketAddr)>) -> Self {
        let this = nodes.iter()
            .find(|(id, _)| *id == this_id)
            .map(|(id, addr)| (*id, NodeConfig {
                addr: *addr,
                spec: String::new(),
                cert: Vec::new(),
                priv_key: Vec::new(),
            }))
            .expect(&format!("this node {} not in config {:?}", this_id, nodes));

        let mut peers = HashMap::new();
        for (id, addr) in nodes {
            if id != this_id {
                peers.insert(id, NodeConfig {
                    addr,
                    spec: String::new(),
                    cert: Vec::new(),
                    priv_key: Vec::new(),
                });
            }
        }

        Self {
            this,
            peers,
            file_dir: PathBuf::new(),
        }
    }

    pub fn get_nodeconfig(&self, id: NodeID) -> &NodeConfig {
        if self.this.0 == id {
            &self.this.1
        } else {
            self.peers.get(&id).unwrap()
        }
    }

    pub fn node_cnt(&self) -> usize {
        self.peers.len() + 1
    }

    pub fn this_node(&self) -> NodeID {
        self.this.0
    }

    pub fn get_master_node(&self) -> NodeID {
        if self.this.1.is_master() {
            return self.this.0;
        }
        *self.peers.iter()
            .find(|(_, config)| config.is_master())
            .map(|(id, _)| id)
            .unwrap()
    }

    pub fn get_worker_nodes(&self) -> HashSet<NodeID> {
        self.peers.iter()
            .filter(|(_, config)| config.is_worker())
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn node_exist(&self, id: NodeID) -> bool {
        self.peers.contains_key(&id) || self.this.0 == id
    }

    pub fn all_nodes_iter(&self) -> impl Iterator<Item = (&NodeID, &NodeConfig)> {
        self.peers.iter().chain(std::iter::once((&self.this.0, &self.this.1)))
    }

    pub fn get_node_addr(&self, node_id: NodeID) -> Option<SocketAddr> {
        self.peers.get(&node_id).map(|node| node.addr)
    }
}

pub fn read_config(this_id: NodeID, file_path: impl AsRef<Path>) -> NodesConfig {
    let config_path = file_path.as_ref().join("files/node_config.yaml");
    let yaml_str = std::fs::read_to_string(config_path)
        .expect("Failed to read config file");
    
    let mut config: HashMap<NodeID, NodeConfig> = serde_yaml::from_str(&yaml_str)
        .expect("Failed to parse config file");

    NodesConfig {
        this: (this_id, config.remove(&this_id).unwrap()),
        peers: config,
        file_dir: file_path.as_ref().to_path_buf(),
    }
} 