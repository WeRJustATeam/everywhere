use crate::result::P2PResult;

pub trait MsgPack: Default {
    fn msg_id(&self) -> u32;
    fn encode_to_vec(&self) -> Vec<u8>;
    fn decode_from(bytes: &[u8]) -> P2PResult<Self>;
    fn decode(bytes: impl AsRef<[u8]>) -> P2PResult<Self> {
        Self::decode_from(bytes.as_ref())
    }
}

pub trait RPCReq: MsgPack {
    type Resp: MsgPack;
    fn encode_resp(resp: &Self::Resp) -> Vec<u8>;
    fn decode_resp(bytes: &[u8]) -> P2PResult<Self::Resp>;
} 