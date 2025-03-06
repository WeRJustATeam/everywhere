use prost::bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;
use crate::result::P2PResult;
use crate::m_p2p::{MsgId, TaskId};

pub trait MsgPack: Sized + Send + Sync + 'static + Default {
    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: Bytes) -> P2PResult<Self>;
    fn msg_id(&self) -> u32;
    
    fn encode_to_vec(&self) -> Vec<u8> {
        self.encode()
    }
    
    fn decode_from(bytes: &[u8]) -> P2PResult<Self> {
        Self::decode(Bytes::copy_from_slice(bytes))
    }
}

pub trait RPCReq: MsgPack {
    type Resp: MsgPack;
    
    fn encode_resp(resp: &Self::Resp) -> Vec<u8> {
        resp.encode()
    }
    
    fn decode_resp(bytes: &[u8]) -> P2PResult<Self::Resp> {
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
}

impl<M: MsgPack> MsgHandler<M> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: RPCReq> RPCCaller<R> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: RPCReq> RPCHandler<R> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> MsgPack for T 
where 
    T: Serialize + DeserializeOwned + Send + Sync + 'static + Debug + Default,
{
    fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
        bincode::deserialize(bytes.as_ref())
            .map_err(|e| crate::result::P2PError::Other(e.to_string()))
    }

    fn msg_id(&self) -> u32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        std::any::type_name::<Self>().hash(&mut hasher);
        hasher.finish() as u32
    }
} 