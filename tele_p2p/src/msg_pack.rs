use prost::bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use tracing::{debug, error, trace};
use crate::result::{P2PResult, P2PError};
use crate::m_p2p::{MsgId, TaskId, NodeID, P2pModule};

pub trait MsgPack: Sized + Send + Sync + 'static + Default {
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: Bytes) -> P2PResult<Self>;
    fn msg_id(&self) -> u32;
    
    fn encode_to_vec(&self) -> Vec<u8> {
        trace!("编码消息: {:?}", std::any::type_name::<Self>());
        self.encode()
    }
    
    fn decode_from(bytes: &[u8]) -> P2PResult<Self> {
        trace!("解码消息: {:?}, 大小: {} 字节", std::any::type_name::<Self>(), bytes.len());
        Self::decode(Bytes::copy_from_slice(bytes))
    }
}

pub trait RPCReq: MsgPack {
    type Resp: MsgPack;
    
    fn encode_resp(resp: &Self::Resp) -> Vec<u8> {
        trace!("编码RPC响应: {:?}", std::any::type_name::<Self::Resp>());
        resp.encode()
    }
    
    fn decode_resp(bytes: &[u8]) -> P2PResult<Self::Resp> {
        trace!("解码RPC响应: {:?}, 大小: {} 字节", std::any::type_name::<Self::Resp>(), bytes.len());
        Self::Resp::decode_from(bytes)
    }
}

pub trait KvResponseExt {
    fn get_task_id(&self) -> TaskId;
}

#[derive(Default)]
pub struct MsgSender<M: MsgPack> {
    _phantom: std::marker::PhantomData<M>,
}

#[derive(Default)]
pub struct MsgHandler<M: MsgPack> {
    _phantom: std::marker::PhantomData<M>,
}

#[derive(Default)]
pub struct RPCCaller<R: RPCReq> {
    _phantom: std::marker::PhantomData<R>,
}

#[derive(Default)]
pub struct RPCHandler<R: RPCReq> {
    _phantom: std::marker::PhantomData<R>,
}

impl<M: MsgPack> MsgSender<M> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
    
    pub async fn send(&self, p2p: &P2pModule, node_id: NodeID, msg: M) -> P2PResult<()> {
        debug!("发送消息 {:?} 到节点 {}", std::any::type_name::<M>(), node_id);
        let encoded = msg.encode_to_vec();
        p2p.p2p_kernel.send(node_id, 0, msg.msg_id(), encoded).await
    }
}

impl<M: MsgPack> MsgHandler<M> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
    
    pub fn regist(
        &self,
        p2p: &P2pModule,
        msg_handler: impl Fn(crate::m_p2p::Responser, M) -> P2PResult<()> + Send + Sync + 'static,
    ) {
        debug!("注册消息处理器: {:?}", std::any::type_name::<M>());
        p2p.regist_dispatch(M::default(), msg_handler);
    }
}

impl<R: RPCReq> RPCCaller<R> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
    
    pub fn regist(&self, p2p: &P2pModule) {
        debug!("注册RPC调用器: {:?}", std::any::type_name::<R>());
        p2p.regist_rpc_send::<R>();
    }
    
    pub async fn call(
        &self,
        p2p: &P2pModule,
        node_id: NodeID,
        req: R,
        timeout: Option<std::time::Duration>,
    ) -> P2PResult<R::Resp> {
        debug!("调用RPC: {:?} 到节点 {}", std::any::type_name::<R>(), node_id);
        p2p.call_rpc::<R>(node_id, req, timeout).await
    }
}

impl<R: RPCReq> RPCHandler<R> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
    
    pub fn regist<F>(&self, p2p: &P2pModule, req_handler: F)
    where
        F: Fn(crate::m_p2p::RPCResponsor<R>, R) -> P2PResult<()> + Send + Sync + 'static,
    {
        debug!("注册RPC处理器: {:?}", std::any::type_name::<R>());
        p2p.regist_rpc_recv::<R, F>(req_handler);
    }
    
    pub async fn call(
        &self,
        p2p: &P2pModule,
        node_id: NodeID,
        req: R,
        timeout: Option<std::time::Duration>,
    ) -> P2PResult<R::Resp> {
        debug!("调用RPC: {:?} 到节点 {}", std::any::type_name::<R>(), node_id);
        p2p.call_rpc::<R>(node_id, req, timeout).await
    }
}

impl<T> MsgPack for T 
where 
    T: Serialize + DeserializeOwned + Send + Sync + 'static + Debug + Default,
{
    fn encode(&self) -> Vec<u8> {
        match bincode::serialize(self) {
            Ok(data) => {
                trace!("序列化成功: {:?}, 大小: {} 字节", std::any::type_name::<Self>(), data.len());
                data
            },
            Err(e) => {
                error!("序列化失败: {:?}, 错误: {}", std::any::type_name::<Self>(), e);
                Vec::new()
            }
        }
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
        bincode::deserialize(bytes.as_ref())
            .map_err(|e| {
                error!("反序列化失败: {:?}, 错误: {}", std::any::type_name::<Self>(), e);
                P2PError::SerdeError(e.to_string())
            })
    }

    fn msg_id(&self) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        std::any::type_name::<Self>().hash(&mut hasher);
        let id = hasher.finish() as u32;
        trace!("生成消息ID: {:?} -> {}", std::any::type_name::<Self>(), id);
        id
    }
} 