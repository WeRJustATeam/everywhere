use std::sync::Arc;
use tele_framework::{
    LogicalModule, WSResult, define_module, define_framework
};
use tele_p2p::{
    config::NodesConfig,
    m_p2p::{P2pModule, P2pModuleView, P2pModuleViewTrait, P2pModuleNewArg},
    result::P2PResult,
    msg_pack::{MsgPack, RPCReq},
};
use tracing_subscriber::EnvFilter;
use paste::paste;
use prost::bytes::Bytes;
use tele_p2p::m_p2p::P2pModuleAccessTrait;

// 定义消息类型
#[derive(Debug, Default)]
struct PingMsg;

impl MsgPack for PingMsg {
    fn encode(&self) -> Vec<u8> {
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
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
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
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
        vec![]
    }

    fn decode(bytes: Bytes) -> P2PResult<Self> {
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
        // 创建模块实例
        let module = Self { view };

        // 注册 RPC 处理器
        module.view.p2p_module().regist_rpc_recv(|resp, _req: PingReq| {
            tokio::spawn(async move {
                println!("Node received ping from node {}", resp.node_id());
                resp.send_resp(PongMsg).await.unwrap();
            });
            Ok(())
        });

        // 等待一会儿让连接建立
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // 发送测试消息
        if module.view.p2p_module().nodes_config.this_node() == 1 {
            match module.view.p2p_module().call_rpc(2, PingReq, None).await {
                Ok(_) => println!("Node 1 received pong from node 2"),
                Err(e) => println!("RPC failed: {}", e),
            }
        }

        Ok(module)
    }

    async fn shutdown(&self) -> WSResult<()> {
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
    // 初始化日志
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // 创建两个节点的配置
    let config1 = NodesConfig::new(
        1,
        vec![
            (1, "127.0.0.1:10001".parse().unwrap()),
            (2, "127.0.0.1:10002".parse().unwrap()),
        ],
    );

    let config2 = NodesConfig::new(
        2,
        vec![
            (1, "127.0.0.1:10001".parse().unwrap()),
            (2, "127.0.0.1:10002".parse().unwrap()),
        ],
    );

    // 创建P2P模块的初始化参数
    let p2p_arg1 = P2pModuleNewArg::new(config1);
    let p2p_arg2 = P2pModuleNewArg::new(config2);
    
    // 创建Demo模块的初始化参数
    let demo_arg = DemoModuleNewArg::new();

    // 创建框架参数
    let framework_args1 = FrameworkArgs {
        p2p_arg: p2p_arg1,
        demo_arg: demo_arg,
    };
    
    let framework_args2 = FrameworkArgs {
        p2p_arg: p2p_arg2,
        demo_arg: DemoModuleNewArg::new(),
    };

    // 创建框架实例 
    let framework1 = Framework::new();
    let framework2 = Framework::new();

    // 初始化框架，传入参数 - 通过trait方法
    framework1.init(framework_args1).await?;
    framework2.init(framework_args2).await?;

    println!("两个节点启动成功，按Ctrl+C退出");

    // 等待 Ctrl+C
    tokio::signal::ctrl_c().await?;

    Ok(())
} 