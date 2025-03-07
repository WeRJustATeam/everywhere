use lazy_static::lazy_static;
use parking_lot::Mutex;
use tokio::sync::mpsc::{self, Sender, Receiver};
use tokio::sync::oneshot;

// 全局共享状态
lazy_static! {
    static ref THE_FIRST: Mutex<bool> = Mutex::new(false);
    static ref RECEIVED: Mutex<bool> = Mutex::new(false);
}

// 重置共享状态
pub fn reset_state() {
    *THE_FIRST.lock() = false;
    *RECEIVED.lock() = false;
}

// 尝试抢占第一个标志，返回是否成功
pub fn try_acquire_first() -> bool {
    let mut lock = THE_FIRST.lock();
    if !*lock {
        *lock = true;
        true
    } else {
        false
    }
}

// 设置收到消息标志
pub fn set_received() {
    *RECEIVED.lock() = true;
}

// 检查是否收到消息
pub fn is_received() -> bool {
    *RECEIVED.lock()
}

// 消息类型
pub enum DemoMsg {
    TryAcquireFirst(oneshot::Sender<bool>),
    SetReceived,
    IsReceived(oneshot::Sender<bool>),
    Reset,
}

// 启动状态管理器
pub fn start_state_manager() -> Sender<DemoMsg> {
    let (tx, mut rx) = mpsc::channel(32);
    
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                DemoMsg::TryAcquireFirst(resp) => {
                    let result = try_acquire_first();
                    let _ = resp.send(result);
                },
                DemoMsg::SetReceived => set_received(),
                DemoMsg::IsReceived(resp) => {
                    let result = is_received();
                    let _ = resp.send(result);
                },
                DemoMsg::Reset => reset_state(),
            }
        }
    });
    
    tx
} 