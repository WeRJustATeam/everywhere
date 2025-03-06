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

use crate::{
    config::NodesConfig,
    result::{P2PError, P2PResult},
    msg_pack::{MsgPack, RPCReq},
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

// 创建一个空的P2PKernel实现用于测试
struct DummyKernel;

#[async_trait]
impl P2PKernel for DummyKernel {
    async fn send_for_response(&self, _nodeid: NodeID, _req_data: Vec<u8>) -> P2PResult<Vec<u8>> {
        Ok(vec![])
    }
    
    async fn send(
        &self,
        _node: NodeID,
        _task_id: TaskId,
        _msg_id: MsgId,
        _req_data: Vec<u8>,
    ) -> P2PResult<()> {
        Ok(())
    }
}

#[async_trait]
impl LogicalModule for P2pModule {
    type View = P2pModuleView;
    type NewArg = P2pModuleNewArg;

    fn name(&self) -> &str {
        "P2pModule"
    }

    async fn init(view: Self::View, arg: Self::NewArg) -> WSResult<Self> {
        // 初始化逻辑
        Ok(Self {
            view: Some(view),
            // 其他字段初始化
            dispatch_map: RwLock::new(HashMap::new()),
            waiting_tasks: crossbeam_skiplist::SkipMap::new(),
            p2p_kernel: Box::new(DummyKernel),
            nodes_config: arg.nodes_config,
            next_task_id: AtomicU32::new(0),
            initialized: Mutex::new(true),
            shutdown: Mutex::new(false),
        })
    }

    async fn shutdown(&self) -> WSResult<()> {
        // 关闭逻辑
        *self.shutdown.lock().unwrap() = true;
        Ok(())
    }
}

impl P2pModule {
    fn regist_dispatch<M, F>(&self, m: M, f: F)
    where
        M: MsgPack,
        F: Fn(Responser, M) -> P2PResult<()> + Send + Sync + 'static,
    {
        let msg_id = m.msg_id();
        
        // 注意：这种方式存在生命周期问题，只是临时解决方案
        // 在实际环境中，应当通过Framework生成新的View
        let static_view: &'static P2pModuleView = unsafe {
            std::mem::transmute::<&P2pModuleView, &'static P2pModuleView>(self.view.as_ref().unwrap())
        };
        
        self.dispatch_map.write().insert(
            msg_id,
            Box::new(move |nid, _p2p, task_id, data| {
                let v = M::decode_from(&data)
                    .map_err(|err| P2PError::InvalidMessage(err.to_string()))?;
                
                let resp = Responser {
                    task_id,
                    node_id: nid,
                    view: unsafe { std::ptr::read(static_view) }, // 这是一个unsafe的临时解决方案
                };
                
                f(resp, v)
            }),
        );
    }

    fn regist_rpc_send<REQ>(&self)
    where
        REQ: RPCReq,
    {
        let resp_type = REQ::Resp::default();
        self.regist_dispatch(resp_type, |resp, v| {
            // 直接获取p2p模块
            let p2p = resp.p2p();
            let cb = p2p
                .waiting_tasks
                .remove(&(resp.task_id, resp.node_id));
            
            if let Some(cell) = cb {
                let _r = cell.value()
                    .lock()
                    .take()
                    .map(|tx| tx.send(v.encode()));
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
        self.regist_dispatch(req_type, move |resp, req| {
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
        _timeout: Option<std::time::Duration>,
    ) -> P2PResult<REQ::Resp> {
        let task_id = self.next_task_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        self.waiting_tasks.insert(
            (task_id, node),
            parking_lot::Mutex::new(Some(tx)),
        );

        let req_data = req.encode();
        self.p2p_kernel.send_for_response(node, req_data).await?;

        let resp_data = rx.await.map_err(|_| P2PError::Other("response channel closed".to_string()))?;
        REQ::Resp::decode_from(&resp_data)
    }

    pub async fn send_resp_impl<RESP>(&self, node_id: NodeID, task_id: TaskId, resp: RESP) -> P2PResult<()>
    where
        RESP: MsgPack,
    {
        let resp_data = resp.encode();
        let msg_id = resp.msg_id();
        self.p2p_kernel.send(node_id, task_id, msg_id, resp_data).await
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
