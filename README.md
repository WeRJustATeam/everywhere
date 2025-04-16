# Everywhere 项目

Everywhere 是一个基于 P2P 网络的分布式系统，提供节点间通信、缓存和 UI 界面等功能。

## 项目结构

- everywhere-core
  - tele_framework
    - 框架核心实现
  - tele_p2p
    - example_basic
      - 基础 P2P 示例，展示节点间通信和 RPC 调用
      - 运行指令：`cd everywhere-core/tele_p2p && cargo run --example basic`
    - example_chat
      - 聊天应用示例，展示如何使用 P2P 网络构建聊天应用
      - 运行指令：`cd everywhere-core/tele_p2p && cargo run --example chat`
    - src
      - 包含 P2P 网络实现的核心代码
  - tele_cache
    - 缓存实现

- everywhere-ui
  - UI 界面实现

- scripts
  - 脚本工具

- tools
  - 开发工具

- waverless
  - 其他组件

## 节点配置

P2P 节点可以通过配置文件进行配置，配置文件位于 `files/node_config.yaml`。节点配置包括以下属性：

- `addr`: 节点地址
- `spec`: 节点规格（如 "master" 或 "worker"）
- `cert`: 节点证书
- `priv_key`: 节点私钥
- `tag`: 节点标签（用于标识和分类节点）

## 开发指南

### 添加新节点

1. 在 `files/node_config.yaml` 中添加新节点配置
2. 使用 `tele_p2p::config::read_config` 函数读取配置
3. 创建 P2P 模块并初始化

### 节点间通信

节点间通信可以通过以下方式实现：

1. 消息发送：使用 `P2pModule::send` 方法
2. RPC 调用：使用 `P2pModule::call_rpc` 方法
3. 消息处理：使用 `P2pModule::regist_dispatch` 和 `P2pModule::regist_rpc_recv` 方法注册消息处理器

## 许可证

[MIT License](LICENSE) 