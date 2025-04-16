use std::sync::Arc;
use tele_framework::{
    LogicalModule, WSResult, define_module, define_framework
};
use tele_p2p::{
    config::NodesConfig,
    m_p2p::{P2pModule, P2pModuleView, P2pModuleViewTrait, P2pModuleNewArg},
    result::P2PResult,
    msg_pack::{MsgPack, RPCReq, MsgSender, RPCCaller, RPCHandler},
};
use tracing::{info, debug, warn, error};
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};
use paste::paste;
use prost::bytes::Bytes;
use tele_p2p::m_p2p::P2pModuleAccessTrait;
use std::time::Duration;
use rand;
use parking_lot::Mutex;
use lazy_static::lazy_static;

// 定义共享变量
lazy_static! {
    static ref THE_FIRST: Mutex<bool> = Mutex::new(false);
    static ref RECEIVED: Mutex<bool> = Mutex::new(false);
}

// 定义消息类型
#[derive(Debug, Default)]
struct PingMsg;

impl MsgPack for PingMsg {
    fn encode(&self) -> Vec<u8> {
        debug!("编码 PingMsg");
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
        debug!("解码 PingMsg");
        Ok(PingMsg)
    }

    fn msg_id(&self) -> u32 {
        1
    }
}

#[derive(Debug, Default)]
struct PongMsg;

impl MsgPack for PongMsg {
    fn encode(&self) -> Vec<u8> {
        debug!("编码 PongMsg");
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
        debug!("解码 PongMsg");
        Ok(PongMsg)
    }

    fn msg_id(&self) -> u32 {
        2
    }
}

// 定义 RPC 请求
#[derive(Debug, Default)]
struct PingReq;

impl MsgPack for PingReq {
    fn encode(&self) -> Vec<u8> {
        debug!("编码 PingReq");
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
        debug!("解码 PingReq");
        Ok(PingReq)
    }

    fn msg_id(&self) -> u32 {
        3
    }
}

impl RPCReq for PingReq {
    type Resp = PongMsg;
}

// 定义演示模块
pub struct DemoModule {
    view: DemoModuleView,
}

// 为DemoModule添加NewArg结构体
#[derive(Debug, Default, Clone)]
pub struct DemoModuleNewArg {
    // 这个结构体目前不需要任何参数
}

impl DemoModuleNewArg {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait::async_trait]
impl LogicalModule for DemoModule {
    type View = DemoModuleView;
    type NewArg = DemoModuleNewArg;

    fn name(&self) -> &str {
        "DemoModule"
    }

    async fn init(view: Self::View, _arg: Self::NewArg) -> WSResult<Self> {
        info!("初始化 DemoModule");
        
        // 重置共享变量
        *THE_FIRST.lock() = false;
        *RECEIVED.lock() = false;
        
        // 从view获取p2p模块及节点ID
        let p2p = view.p2p_module();
        let this_node = p2p.nodes_config.this_node();
        info!("当前节点ID: {}", this_node);
        
        // 注册 RPC 处理器
        let rpc_handler = RPCHandler::<PingReq>::new();
        let this_node_copy = this_node; // 复制节点ID以便在闭包中使用
        rpc_handler.regist(p2p, move |resp, _req: PingReq| {
            info!("节点 {} 接收到来自节点 {} 的ping请求", this_node_copy, resp.node_id());
            tokio::spawn(async move {
                debug!("节点 {} 处理来自节点 {} 的ping请求", resp.node_id(), resp.node_id());
                // 设置接收标志为true
                *RECEIVED.lock() = true;
                info!("节点 {} 设置RECEIVED为true", resp.node_id());
                
                // 发送响应
                match resp.send_resp(PongMsg).await {
                    Ok(_) => info!("节点 {} 成功发送pong响应", resp.node_id()),
                    Err(e) => error!("节点 {} 发送pong响应失败: {}", resp.node_id(), e),
                }
            });
            Ok(())
        });
        
        // 克隆P2P模块以便在任务中使用
        let view2 = view.clone();
        // 启动争抢任务
        tokio::spawn(async move {
            // 先等待一点随机时间，增加竞争的随机性
            debug!("节点 {} 将等待 {}ms 后尝试争抢", this_node, 10);
            tokio::time::sleep(Duration::from_secs(10)).await;
            
            // 尝试抢占THE_FIRST
            let is_first = {
                let mut lock = THE_FIRST.lock();
                if !*lock {
                    *lock = true;
                    true
                } else {
                    false
                }
            };
            
            if is_first {
                info!("✅ 节点 {} 成功争抢到THE_FIRST标志", this_node);
                // 获取对方节点ID
                let target_node = if this_node == 1 { 2 } else { 1 };

                // 发送请求到另一个节点
                info!("节点 {} 将在5秒内尝试发送请求到节点 {}", this_node, target_node);
                
                // 使用RPCCaller发送请求
                let rpc_caller = RPCCaller::<PingReq>::new();
                rpc_caller.regist(view2.p2p_module());
                
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    rpc_caller.call(view2.p2p_module(), target_node, PingReq, Some(Duration::from_secs(3)))
                ).await {
                    Ok(result) => {
                        match result {
                            Ok(_) => {
                                info!("✅ 节点 {} 成功发送请求到节点 {} 并收到响应", this_node, target_node);
                                // 检查是否收到响应
                                if *RECEIVED.lock() {
                                    info!("✅ 确认RECEIVED已设置为true");
                                } else {
                                    warn!("❌ 警告：RECEIVED未设置为true");
                                }
                            },
                            Err(e) => error!("❌ 节点 {} RPC调用失败: {}", this_node, e),
                        }
                    },
                    Err(_) => error!("❌ 节点 {} 发送请求超时", this_node),
                }
            } else {
                info!("❌ 节点 {} 未争抢到THE_FIRST标志", this_node);
                // 等待5秒，看是否收到请求
                info!("节点 {} 将等待5秒，查看是否收到请求", this_node);
                if let Err(_) = tokio::time::timeout(
                    Duration::from_secs(5),
                    async {
                        loop {
                            if *RECEIVED.lock() {
                                info!("✅ 节点 {} 确认已接收到请求并设置RECEIVED为true", this_node);
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                ).await {
                    warn!("❌ 节点 {} 等待请求超时，未收到请求", this_node);
                }
            }
        });

        // 完成所有操作后，创建模块实例
        let module = Self { view };
        
        info!("DemoModule 初始化完成");

        Ok(module)
    }

    async fn shutdown(&self) -> WSResult<()> {
        info!("关闭 DemoModule");
        Ok(())
    }
}

// 定义模块和框架
define_module!(DemoModule, (p2p, P2pModule));

define_framework! {
    p2p: P2pModule,
    demo: DemoModule
}

#[tokio::main]
async fn main() -> WSResult<()> {
    // 初始化日志，添加更详细的配置
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("tele_p2p=debug".parse().unwrap()))
        .with_span_events(FmtSpan::CLOSE)
        .finish();
    
    tracing::subscriber::set_global_default(subscriber)
        .expect("设置全局日志订阅器失败");
    
    info!("启动P2P示例程序");

    // 创建两个节点的配置
    let config1 = NodesConfig::new(
        1,
        vec![
            (1, "127.0.0.1:10001".parse().unwrap()),
            (2, "127.0.0.1:10002".parse().unwrap()),
        ],
    );
    info!("创建节点1配置: {:?}", config1.this);

    let config2 = NodesConfig::new(
        2,
        vec![
            (1, "127.0.0.1:10001".parse().unwrap()),
            (2, "127.0.0.1:10002".parse().unwrap()),
        ],
    );
    info!("创建节点2配置: {:?}", config2.this);

    // 创建P2P模块的初始化参数
    let p2p_arg1 = P2pModuleNewArg::new(config1);
    let p2p_arg2 = P2pModuleNewArg::new(config2);
    
    // 创建Demo模块的初始化参数
    let demo_arg = DemoModuleNewArg::new();

    // 创建框架参数
    let framework_args1 = FrameworkArgs {
        p2p_arg: p2p_arg1,
        demo_arg: demo_arg.clone(),
    };
    
    let framework_args2 = FrameworkArgs {
        p2p_arg: p2p_arg2,
        demo_arg: demo_arg,
    };

    // 创建框架实例 
    let framework1 = Framework::new();
    let framework2 = Framework::new();
    info!("创建框架实例完成");

    // 初始化框架，传入参数 - 通过trait方法
    info!("初始化节点1框架");
    framework1.init(framework_args1).await?;
    info!("初始化节点2框架");
    framework2.init(framework_args2).await?;

    info!("两个节点启动成功，等待10秒钟让节点互联...");
    // 添加10秒等待时间，让节点有足够的时间建立连接
    tokio::time::sleep(Duration::from_secs(10)).await;
    info!("等待结束，节点应该已经互联");

    info!("两个节点启动成功，按Ctrl+C退出");

    // 等待 Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("接收到Ctrl+C信号，程序退出");

    Ok(())
} 