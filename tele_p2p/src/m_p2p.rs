use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

use tele_framework::{LogicalModule, WSResult};
use parking_lot::RwLock;
use prost::bytes::Bytes;
use async_trait::async_trait;
use paste::paste;
use tracing::{debug, error, info, warn};

use crate::{
    config::NodesConfig,
    result::{P2PError, P2PResult},
    msg_pack::{MsgPack, RPCReq},
    m_p2p_quic::P2PQuicNode,
};

pub type TaskId = u32;
pub type MsgId = u32;
pub type NodeID = u32;

#[async_trait]
pub trait P2PKernel: Send + Sync {
    async fn send_for_response(&self, nodeid: NodeID, req_data: Vec<u8>) -> P2PResult<Vec<u8>>;
    async fn send(
        &self,
        node: NodeID,
        task_id: TaskId,
        msg_id: MsgId,
        req_data: Vec<u8>,
    ) -> P2PResult<()>;
    
    // 添加start方法到trait定义
    async fn start(&self) -> P2PResult<()>;
}

pub enum DispatchPayload<M: MsgPack> {
    Remote(Bytes),
    Local(M),
}

pub struct Responser {
    task_id: TaskId,
    pub node_id: NodeID,
    view: P2pModuleView, // 使用view而不是指针
}

impl Responser {
    pub async fn send_resp<RESP>(&self, resp: RESP) -> P2PResult<()>
    where
        RESP: MsgPack,
    {
        // 直接获取p2p模块并调用方法
        self.p2p().send_resp_impl(self.node_id, self.task_id, resp).await
    }

    pub fn node_id(&self) -> NodeID {
        self.node_id
    }

    pub fn task_id(&self) -> TaskId {
        self.task_id
    }
    
    // 修改为通过view直接访问p2p
    pub fn p2p(&self) -> &P2pModule {
        // 直接使用unsafe获取p2p模块，跳过安全检查
        unsafe {
            // 直接通过unsafe方式获取P2pModule引用
            std::mem::transmute(&self.view)
        }
    }
}

pub struct RPCResponsor<R: RPCReq> {
    _phantom: PhantomData<R>,
    responsor: Responser,
}

impl<R: RPCReq> RPCResponsor<R> {
    pub async fn send_resp(&self, resp: R::Resp) -> P2PResult<()> {
        self.responsor.send_resp(resp).await
    }

    pub fn node_id(&self) -> NodeID {
        self.responsor.node_id
    }

    pub fn task_id(&self) -> TaskId {
        self.responsor.task_id
    }
}

pub struct P2pModule {
    dispatch_map: RwLock<
        HashMap<
            MsgId,
            Box<dyn Fn(NodeID, &Self, TaskId, Vec<u8>) -> P2PResult<()> + Send + Sync>,
        >,
    >,
    waiting_tasks: crossbeam_skiplist::SkipMap<
        (TaskId, NodeID),
        parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<Vec<u8>>>>,
    >,
    pub p2p_kernel: Box<dyn P2PKernel>,
    pub nodes_config: NodesConfig,
    pub next_task_id: AtomicU32,
    pub initialized: Mutex<bool>,
    pub shutdown: Mutex<bool>,
    pub view: Option<P2pModuleView>,
}

#[async_trait]
impl LogicalModule for P2pModule {
    type View = P2pModuleView;
    type NewArg = P2pModuleNewArg;

    fn name(&self) -> &str {
        "P2pModule"
    }

    async fn init(view: Self::View, arg: Self::NewArg) -> WSResult<Self> {
        info!("初始化 P2pModule");
        
        // 创建P2PQuicNode替代DummyKernel
        info!("创建P2PQuicNode作为P2P内核");
        let mut p2p_quic_node = P2PQuicNode::new(arg.nodes_config.clone());
        
        // 设置view
        info!("设置P2pModuleView到P2PQuicNode");
        p2p_quic_node.set_view(view.clone());
        
        // 初始化模块
        let module = Self {
            view: Some(view),
            dispatch_map: RwLock::new(HashMap::new()),
            waiting_tasks: crossbeam_skiplist::SkipMap::new(),
            p2p_kernel: Box::new(p2p_quic_node),
            nodes_config: arg.nodes_config,
            next_task_id: AtomicU32::new(0),
            initialized: Mutex::new(true),
            shutdown: Mutex::new(false),
        };
        
        // 启动P2P内核
        info!("启动P2P内核");
        if let Err(e) = module.p2p_kernel.start().await {
            error!("启动P2P内核失败: {}", e);
            // 转换错误类型
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other, 
                format!("启动P2P内核失败: {}", e)
            )));
        }
        
        Ok(module)
    }

    async fn shutdown(&self) -> WSResult<()> {
        // 关闭逻辑
        info!("关闭 P2pModule");
        *self.shutdown.lock().unwrap() = true;
        Ok(())
    }
}

impl P2pModule {
    // 获取节点地址
    pub fn get_addr_by_id(&self, node_id: NodeID) -> P2PResult<std::net::SocketAddr> {
        match self.nodes_config.peers.get(&node_id) {
            Some(node_config) => Ok(node_config.addr),
            None => {
                error!("找不到节点ID: {}", node_id);
                Err(P2PError::Other(format!("找不到节点ID: {}", node_id)))
            }
        }
    }

    // 找到对应地址的节点ID
    pub fn find_peer_id(&self, addr: &std::net::SocketAddr) -> Option<NodeID> {
        self.nodes_config.peers.iter().find_map(|(id, node_config)| {
            if node_config.addr == *addr {
                debug!("找到地址 {:?} 对应的节点ID: {}", addr, id);
                Some(*id)
            } else {
                None
            }
        })
    }

    pub fn regist_dispatch<M, F>(&self, m: M, f: F)
    where
        M: MsgPack,
        F: Fn(Responser, M) -> P2PResult<()> + Send + Sync + 'static,
    {
        let msg_id = m.msg_id();
        info!("注册消息处理器，消息ID: {}", msg_id);
        
        // 注意：这种方式存在生命周期问题，只是临时解决方案
        // 在实际环境中，应当通过Framework生成新的View
        let static_view: &'static P2pModuleView = unsafe {
            std::mem::transmute::<&P2pModuleView, &'static P2pModuleView>(self.view.as_ref().unwrap())
        };
        
        self.dispatch_map.write().insert(
            msg_id,
            Box::new(move |nid, _p2p, task_id, data| {
                debug!("处理来自节点 {} 的消息，任务ID: {}, 消息ID: {}", nid, task_id, msg_id);
                let v = M::decode_from(&data)
                    .map_err(|err| {
                        error!("解码消息失败: {}", err);
                        P2PError::InvalidMessage(err.to_string())
                    })?;
                
                let resp = Responser {
                    task_id,
                    node_id: nid,
                    view: unsafe { std::ptr::read(static_view) }, // 这是一个unsafe的临时解决方案
                };
                
                f(resp, v)
            }),
        );
    }

    // 分发收到的消息
    pub fn dispatch(&self, node_id: NodeID, msg_id: MsgId, task_id: TaskId, bytes: Vec<u8>) {
        debug!("分发消息: 节点={}, 消息ID={}, 任务ID={}", node_id, msg_id, task_id);
        
        let dispatch_map = self.dispatch_map.read();
        match dispatch_map.get(&msg_id) {
            Some(handler) => {
                if let Err(e) = handler(node_id, self, task_id, bytes) {
                    error!("消息处理失败: {}", e);
                }
            }
            None => {
                warn!("未找到消息ID {} 的处理器", msg_id);
            }
        }
    }

    pub fn regist_rpc_send<REQ>(&self)
    where
        REQ: RPCReq,
    {
        let resp_type = REQ::Resp::default();
        info!("注册RPC响应处理器: {:?}", std::any::type_name::<REQ::Resp>());
        
        self.regist_dispatch(resp_type, |resp, v| {
            // 直接获取p2p模块
            debug!("处理RPC响应: 节点={}, 任务ID={}", resp.node_id, resp.task_id);
            let p2p = resp.p2p();
            let cb = p2p
                .waiting_tasks
                .remove(&(resp.task_id, resp.node_id));
            
            if let Some(cell) = cb {
                let _r = cell.value()
                    .lock()
                    .take()
                    .map(|tx| {
                        debug!("发送RPC响应");
                        tx.send(v.encode())
                    });
            } else {
                warn!("找不到等待的RPC任务: ({}, {})", resp.task_id, resp.node_id);
            }
            
            Ok(())
        });
    }

    pub fn regist_rpc_recv<REQ, F>(&self, req_handler: F)
    where
        REQ: RPCReq + Default,
        F: Fn(RPCResponsor<REQ>, REQ) -> P2PResult<()> + Send + Sync + 'static,
    {
        let req_type = REQ::default();
        info!("注册RPC请求处理器: {:?}", std::any::type_name::<REQ>());
        
        self.regist_dispatch(req_type, move |resp, req| {
            debug!("处理RPC请求: 节点={}, 任务ID={}", resp.node_id, resp.task_id);
            let rpc_resp = RPCResponsor {
                _phantom: PhantomData,
                responsor: resp,
            };
            req_handler(rpc_resp, req)
        });
    }

    pub async fn call_rpc<REQ: RPCReq>(
        &self,
        node: NodeID,
        req: REQ,
        timeout: Option<std::time::Duration>,
    ) -> P2PResult<REQ::Resp> {
        let task_id = self.next_task_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        debug!("发起RPC调用: 节点={}, 任务ID={}, 类型={:?}", 
               node, task_id, std::any::type_name::<REQ>());
        
        self.waiting_tasks.insert(
            (task_id, node),
            parking_lot::Mutex::new(Some(tx)),
        );

        let req_data = req.encode();
        if let Err(e) = self.p2p_kernel.send(node, task_id, req.msg_id(), req_data).await {
            error!("发送RPC请求失败: {}", e);
            return Err(e);
        }

        // 使用超时机制
        let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(30));
        let result = tokio::time::timeout(timeout_duration, rx).await;
        
        match result {
            Ok(Ok(resp_data)) => {
                debug!("RPC调用成功: 节点={}, 任务ID={}", node, task_id);
                REQ::Resp::decode_from(&resp_data)
                    .map_err(|e| {
                        error!("解码RPC响应失败: {}", e);
                        P2PError::InvalidMessage(e.to_string())
                    })
            },
            Ok(Err(_)) => {
                error!("RPC响应通道错误: 节点={}, 任务ID={}", node, task_id);
                Err(P2PError::Other("RPC响应通道错误".to_string()))
            },
            Err(_) => {
                error!("RPC调用超时: 节点={}, 任务ID={}", node, task_id);
                // 清理等待任务
                self.waiting_tasks.remove(&(task_id, node));
                Err(P2PError::Timeout(format!("RPC调用超时: 节点={}, 任务ID={}", node, task_id)))
            }
        }
    }

    // 发送响应实现
    pub async fn send_resp_impl<RESP>(&self, node: NodeID, task_id: TaskId, resp: RESP) -> P2PResult<()>
    where
        RESP: MsgPack,
    {
        debug!("发送响应: 节点={}, 任务ID={}", node, task_id);
        let resp_data = resp.encode();
        self.p2p_kernel.send(node, task_id, resp.msg_id(), resp_data).await
    }
}

tele_framework::define_module!(P2pModule, (p2p, P2pModule));

// 添加P2pModuleNewArg结构体定义
pub struct P2pModuleNewArg {
    pub nodes_config: NodesConfig,
}

impl P2pModuleNewArg {
    pub fn new(nodes_config: NodesConfig) -> Self {
        Self { nodes_config }
    }
}

pub struct TestModuleA {
    _phantom: std::marker::PhantomData<()>,
    pub initialized: Mutex<bool>,
    pub shutdown: Mutex<bool>,
}

pub struct TestModuleAView {
    // 视图的字段
}

pub struct TestModuleB {
    _phantom: std::marker::PhantomData<()>,
    pub initialized: Mutex<bool>,
    pub shutdown: Mutex<bool>,
}

pub struct TestModuleBView {
    // 视图的字段
}

#[async_trait]
impl LogicalModule for TestModuleA {
    type View = TestModuleAView;
    type NewArg = ();

    fn name(&self) -> &str {
        "TestModuleA"
    }

    async fn init(view: Self::View, _arg: Self::NewArg) -> WSResult<Self> {
        Ok(Self {
            _phantom: std::marker::PhantomData,
            initialized: Mutex::new(true),
            shutdown: Mutex::new(false),
        })
    }

    async fn shutdown(&self) -> WSResult<()> {
        *self.shutdown.lock().unwrap() = true;
        Ok(())
    }
}

#[async_trait]
impl LogicalModule for TestModuleB {
    type View = TestModuleBView;
    type NewArg = ();

    fn name(&self) -> &str {
        "TestModuleB"
    }

    async fn init(view: Self::View, _arg: Self::NewArg) -> WSResult<Self> {
        Ok(Self {
            _phantom: std::marker::PhantomData,
            initialized: Mutex::new(true),
            shutdown: Mutex::new(false),
        })
    }

    async fn shutdown(&self) -> WSResult<()> {
        *self.shutdown.lock().unwrap() = true;
        Ok(())
    }
}
